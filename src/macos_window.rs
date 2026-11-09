use std::{ffi::c_char, sync::OnceLock};

use objc2::{
    MainThreadMarker,
    rc::Retained,
    runtime::{AnyClass, AnyObject, Bool, Imp, Method, Sel},
    sel,
};
use objc2_app_kit::{
    NSApplication, NSPopUpMenuWindowLevel, NSScreen, NSView, NSWindow, NSWindowCollectionBehavior,
};
use objc2_foundation::NSRect;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

pub fn screens() -> Result<Vec<NSRect>, String> {
    let mtm = MainThreadMarker::new().ok_or("screen lookup requires the main thread")?;
    let screens: Vec<_> = NSScreen::screens(mtm)
        .iter()
        .map(|screen| screen.frame())
        .collect();
    if screens.is_empty() {
        return Err("no Mac displays are available".into());
    }
    Ok(screens)
}

fn root_window(root: &Window) -> Result<Retained<NSWindow>, String> {
    let handle = root.window_handle().map_err(|error| error.to_string())?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return Err("root window has no AppKit handle".into());
    };
    // The borrowed winit window keeps its NSView alive for this lookup.
    let view: Retained<NSView> = unsafe { Retained::retain(handle.ns_view.as_ptr().cast()) }
        .ok_or("cannot retain root NSView")?;
    view.window().ok_or("root NSView has no NSWindow".into())
}

fn child_window(title: &str) -> Result<Retained<NSWindow>, String> {
    let mtm = MainThreadMarker::new().ok_or("window lookup requires the main thread")?;
    NSApplication::sharedApplication(mtm)
        .windows()
        .iter()
        .find(|window| window.title().to_string() == title)
        .ok_or_else(|| format!("cannot find overlay window {title}"))
}

unsafe extern "C" {
    fn class_replaceMethod(
        class: &AnyClass,
        selector: Sel,
        implementation: Imp,
        types: *const c_char,
    ) -> Option<Imp>;
    fn method_getTypeEncoding(method: &Method) -> *const c_char;
}

type MouseDown = unsafe extern "C-unwind" fn(&NSView, Sel, *mut AnyObject);
static ORIGINAL_MOUSE_DOWN: OnceLock<MouseDown> = OnceLock::new();

extern "C-unwind" fn cannot_focus(_window: &NSWindow, _selector: Sel) -> Bool {
    Bool::NO
}

extern "C-unwind" fn show_without_focus(
    window: &NSWindow,
    _selector: Sel,
    _sender: *mut AnyObject,
) {
    window.orderFrontRegardless();
}

extern "C-unwind" fn accept_mouse(_view: &NSView, _selector: Sel, _event: *mut AnyObject) -> Bool {
    Bool::YES
}

extern "C-unwind" fn mouse_down(view: &NSView, selector: Sel, event: *mut AnyObject) {
    if let Some(mtm) = MainThreadMarker::new() {
        NSApplication::sharedApplication(mtm).preventWindowOrdering();
    }
    // Keep winit's input handling while cancelling AppKit's click activation.
    if let Some(original) = ORIGINAL_MOUSE_DOWN.get() {
        unsafe { original(view, selector, event) };
    }
}

unsafe fn override_method(
    class: &AnyClass,
    selector: Sel,
    implementation: Imp,
) -> Result<(), String> {
    let method = class
        .instance_method(selector)
        .ok_or_else(|| format!("reminder class has no {selector}"))?;
    // Keep AppKit's KVO class identity and the original method's type encoding.
    unsafe {
        class_replaceMethod(
            class,
            selector,
            implementation,
            method_getTypeEncoding(method),
        )
    };
    Ok(())
}

fn preserve_keyboard_focus(window: &NSWindow) -> Result<(), String> {
    let view = window
        .contentView()
        .ok_or("reminder window has no content view")?;
    let original = view
        .class()
        .instance_method(sel!(mouseDown:))
        .ok_or("reminder view has no mouseDown:")?
        .implementation();
    // Only this overlay process owns these winit classes; all calls are on its UI thread.
    unsafe {
        ORIGINAL_MOUSE_DOWN.get_or_init(|| std::mem::transmute::<Imp, MouseDown>(original));
        let cannot_focus =
            std::mem::transmute::<extern "C-unwind" fn(&NSWindow, Sel) -> Bool, Imp>(cannot_focus);
        override_method(window.class(), sel!(canBecomeKeyWindow), cannot_focus)?;
        override_method(window.class(), sel!(canBecomeMainWindow), cannot_focus)?;
        override_method(
            window.class(),
            sel!(makeKeyAndOrderFront:),
            std::mem::transmute::<extern "C-unwind" fn(&NSWindow, Sel, *mut AnyObject), Imp>(
                show_without_focus,
            ),
        )?;
        let accept_mouse = std::mem::transmute::<
            extern "C-unwind" fn(&NSView, Sel, *mut AnyObject) -> Bool,
            Imp,
        >(accept_mouse);
        override_method(view.class(), sel!(acceptsFirstMouse:), accept_mouse)?;
        override_method(
            view.class(),
            sel!(shouldDelayWindowOrderingForEvent:),
            accept_mouse,
        )?;
        override_method(
            view.class(),
            sel!(mouseDown:),
            std::mem::transmute::<extern "C-unwind" fn(&NSView, Sel, *mut AnyObject), Imp>(
                mouse_down,
            ),
        )?;
    }
    Ok(())
}

fn configure(window: &NSWindow, frame: NSRect) -> Result<(), String> {
    preserve_keyboard_focus(window)?;
    // Join an existing fullscreen Space without creating one for the reminder.
    window.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::FullScreenDisallowsTiling,
    );
    window.setLevel(NSPopUpMenuWindowLevel);
    window.setHasShadow(false);
    if window.frame() != frame {
        window.setFrame_display(frame, true);
    }
    Ok(())
}

pub fn configure_root(root: &Window, frame: NSRect) -> Result<(), String> {
    let window = root_window(root)?;
    configure(&window, frame)
}

pub fn root_ready(root: &Window, frame: NSRect) -> bool {
    root_window(root).is_ok_and(|window| window.isVisible() && window.frame() == frame)
}

pub fn configure_child(title: &str, frame: NSRect) -> Result<(), String> {
    let window = child_window(title)?;
    configure(&window, frame)
}

pub fn show_child(title: &str, frame: NSRect) -> Result<bool, String> {
    let window = child_window(title)?;
    if !window.isVisible() {
        window.orderFrontRegardless();
    }
    Ok(window.isVisible() && window.frame() == frame)
}
