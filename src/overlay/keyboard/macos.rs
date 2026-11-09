use std::{
    ffi::c_void,
    ptr,
    sync::mpsc::Sender,
    time::{Duration, Instant},
};

use eframe::egui;

use super::KeyEvent;

const KEYS: [u32; 3] = [36, 76, 53];
const SIGNATURE: u32 = u32::from_be_bytes(*b"BRsk");
const KEYBOARD: u32 = u32::from_be_bytes(*b"keyb");

#[repr(C)]
struct HotKeyId {
    signature: u32,
    id: u32,
}

#[repr(C)]
struct EventType {
    class: u32,
    kind: u32,
}

type NativeRef = *mut c_void;

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn GetApplicationEventTarget() -> NativeRef;
    fn InstallEventHandler(
        target: NativeRef,
        handler: unsafe extern "C" fn(NativeRef, NativeRef, NativeRef) -> i32,
        count: usize,
        events: *const EventType,
        data: NativeRef,
        result: *mut NativeRef,
    ) -> i32;
    fn RemoveEventHandler(handler: NativeRef) -> i32;
    fn RegisterEventHotKey(
        key: u32,
        modifiers: u32,
        id: HotKeyId,
        target: NativeRef,
        options: u32,
        result: *mut NativeRef,
    ) -> i32;
    fn UnregisterEventHotKey(key: NativeRef) -> i32;
    fn GetEventParameter(
        event: NativeRef,
        name: u32,
        desired_type: u32,
        actual_type: *mut u32,
        buffer_size: usize,
        actual_size: *mut usize,
        data: NativeRef,
    ) -> i32;
    fn GetEventKind(event: NativeRef) -> u32;
    fn GetEventTime(event: NativeRef) -> f64;
    fn GetCurrentEventTime() -> f64;
    fn GetKeys(keys: *mut u32);
}

struct Handler {
    sender: Sender<KeyEvent>,
    context: egui::Context,
}

pub(super) struct Keyboard {
    handler: NativeRef,
    keys: [NativeRef; 3],
    data: Box<Handler>,
}

impl Keyboard {
    pub(super) fn new(sender: Sender<KeyEvent>, context: egui::Context) -> Result<Self, String> {
        let mut keyboard = Self {
            handler: ptr::null_mut(),
            keys: [ptr::null_mut(); 3],
            data: Box::new(Handler { sender, context }),
        };
        let events = [
            EventType {
                class: KEYBOARD,
                kind: 5,
            },
            EventType {
                class: KEYBOARD,
                kind: 6,
            },
        ];
        // Carbon dispatches this handler on the main event loop; its boxed data stays put.
        let target = unsafe { GetApplicationEventTarget() };
        let status = unsafe {
            InstallEventHandler(
                target,
                handle_event,
                events.len(),
                events.as_ptr(),
                ptr::from_mut(keyboard.data.as_mut()).cast(),
                &mut keyboard.handler,
            )
        };
        if status != 0 {
            return Err(format!(
                "cannot install break shortcut handler (macOS error {status})"
            ));
        }
        for (index, key) in KEYS.into_iter().enumerate() {
            let status = unsafe {
                RegisterEventHotKey(
                    key,
                    0,
                    HotKeyId {
                        signature: SIGNATURE,
                        id: index as u32,
                    },
                    target,
                    1,
                    &mut keyboard.keys[index],
                )
            };
            if status != 0 {
                return Err(format!(
                    "cannot reserve break shortcut key {key} (macOS error {status})"
                ));
            }
        }
        Ok(keyboard)
    }

    pub(super) fn held(&self) -> [bool; 3] {
        let mut keys = [0u32; 4];
        unsafe { GetKeys(keys.as_mut_ptr()) };
        let bytes = keys.map(u32::to_ne_bytes).concat();
        KEYS.map(|key| bytes[key as usize / 8] & (1 << (key % 8)) != 0)
    }
}

impl Drop for Keyboard {
    fn drop(&mut self) {
        for key in self.keys.into_iter().filter(|key| !key.is_null()) {
            unsafe { UnregisterEventHotKey(key) };
        }
        if !self.handler.is_null() {
            unsafe { RemoveEventHandler(self.handler) };
        }
    }
}

unsafe extern "C" fn handle_event(_: NativeRef, event: NativeRef, data: NativeRef) -> i32 {
    let mut id = HotKeyId {
        signature: 0,
        id: 0,
    };
    let mut size = 0;
    let status = unsafe {
        GetEventParameter(
            event,
            u32::from_be_bytes(*b"----"),
            u32::from_be_bytes(*b"hkid"),
            ptr::null_mut(),
            size_of::<HotKeyId>(),
            &mut size,
            ptr::from_mut(&mut id).cast(),
        )
    };
    if status != 0
        || size != size_of::<HotKeyId>()
        || id.signature != SIGNATURE
        || id.id >= KEYS.len() as u32
    {
        return -9874; // eventNotHandledErr: allow unrelated hotkey handlers to run.
    }
    let handler = unsafe { &*data.cast::<Handler>() };
    let now = Instant::now();
    let age = unsafe { GetCurrentEventTime() - GetEventTime(event) };
    let age = Duration::try_from_secs_f64(age.max(0.0)).unwrap_or(Duration::ZERO);
    let _ = handler.sender.send(KeyEvent {
        key: id.id as usize,
        pressed: unsafe { GetEventKind(event) } == 5,
        at: now.checked_sub(age).unwrap_or(now),
    });
    handler.context.request_repaint();
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[link(name = "Carbon", kind = "framework")]
    unsafe extern "C" {
        fn CreateEvent(
            allocator: *const c_void,
            class: u32,
            kind: u32,
            time: f64,
            flags: u32,
            event: *mut NativeRef,
        ) -> i32;
        fn SetEventParameter(
            event: NativeRef,
            name: u32,
            kind: u32,
            size: usize,
            data: *const c_void,
        ) -> i32;
        fn ReleaseEvent(event: NativeRef);
    }

    #[test]
    fn native_handler_preserves_key_transitions_and_original_event_time() {
        let (sender, receiver) = mpsc::channel();
        let mut handler = Handler {
            sender,
            context: egui::Context::default(),
        };
        for key in 0..KEYS.len() {
            for (kind, pressed) in [(5, true), (6, false)] {
                let before = Instant::now();
                let mut event = ptr::null_mut();
                let id = HotKeyId {
                    signature: SIGNATURE,
                    id: key as u32,
                };
                unsafe {
                    assert_eq!(
                        CreateEvent(
                            ptr::null(),
                            KEYBOARD,
                            kind,
                            GetCurrentEventTime() - 2.0,
                            0,
                            &mut event
                        ),
                        0
                    );
                    assert_eq!(
                        SetEventParameter(
                            event,
                            u32::from_be_bytes(*b"----"),
                            u32::from_be_bytes(*b"hkid"),
                            size_of::<HotKeyId>(),
                            ptr::from_ref(&id).cast()
                        ),
                        0
                    );
                    assert_eq!(
                        handle_event(ptr::null_mut(), event, ptr::from_mut(&mut handler).cast()),
                        0
                    );
                    ReleaseEvent(event);
                }
                let received = receiver.try_recv().unwrap();
                assert_eq!(received.key, key);
                assert_eq!(received.pressed, pressed);
                assert!(before.duration_since(received.at) >= Duration::from_millis(1900));
                assert!(before.duration_since(received.at) < Duration::from_millis(2100));
            }
        }
    }
}
