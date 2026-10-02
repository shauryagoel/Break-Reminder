use objc2::{MainThreadMarker, rc::Retained};
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

fn configure(window: &NSWindow, frame: NSRect) {
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
}

pub fn configure_root(root: &Window, frame: NSRect) -> Result<(), String> {
    let window = root_window(root)?;
    configure(&window, frame);
    Ok(())
}

pub fn root_ready(root: &Window, frame: NSRect) -> bool {
    root_window(root).is_ok_and(|window| window.isVisible() && window.frame() == frame)
}

pub fn configure_child(title: &str, frame: NSRect) -> Result<(), String> {
    let window = child_window(title)?;
    configure(&window, frame);
    Ok(())
}

pub fn show_child(title: &str, frame: NSRect) -> Result<bool, String> {
    let window = child_window(title)?;
    if !window.isVisible() {
        window.orderFrontRegardless();
    }
    Ok(window.isVisible() && window.frame() == frame)
}
