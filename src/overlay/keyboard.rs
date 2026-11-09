use std::{
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

use eframe::egui;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "linux")]
use linux::Keyboard;
#[cfg(target_os = "macos")]
use macos::Keyboard;

struct KeyEvent {
    key: usize,
    pressed: bool,
    at: Instant,
}

struct SkipGuard {
    visible_at: Option<Instant>,
    held: [bool; 3],
}

pub(super) struct Shortcuts {
    native: Keyboard,
    events: Receiver<KeyEvent>,
    guard: SkipGuard,
}

impl Shortcuts {
    pub(super) fn new(context: egui::Context) -> Result<Self, String> {
        let (sender, events) = mpsc::channel();
        let native = Keyboard::new(sender, context)?;
        let guard = SkipGuard {
            visible_at: None,
            held: native.held(),
        };
        Ok(Self {
            native,
            events,
            guard,
        })
    }

    pub(super) fn visible(&mut self, now: Instant) {
        self.guard.visible_at.get_or_insert(now);
    }

    pub(super) fn poll(&mut self, started: Option<Instant>) -> Result<bool, String> {
        #[cfg(target_os = "linux")]
        self.native.poll()?;
        let mut skip = false;
        for event in self.events.try_iter() {
            skip |= self.guard.key(event, started);
        }
        // A key held before registration may be released without a hotkey event.
        for (held, physical) in self.guard.held.iter_mut().zip(self.native.held()) {
            *held &= physical;
        }
        Ok(skip)
    }
}

impl SkipGuard {
    fn key(&mut self, event: KeyEvent, started: Option<Instant>) -> bool {
        let fresh = event.pressed && !self.held[event.key];
        self.held[event.key] = event.pressed;
        fresh
            && self.visible_at.is_some_and(|visible| {
                event.at.saturating_duration_since(visible) >= Duration::from_secs(1)
            })
            && started.is_some_and(|started| event.at >= started)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_wait_one_second_and_require_a_fresh_press() {
        let now = Instant::now();
        for key in 0..3 {
            let mut guard = SkipGuard {
                visible_at: Some(now),
                held: [false; 3],
            };
            let event = |pressed, millis| KeyEvent {
                key,
                pressed,
                at: now + Duration::from_millis(millis),
            };
            assert!(!guard.key(event(true, 999), Some(now)));
            assert!(!guard.key(event(true, 1000), Some(now)));
            assert!(!guard.key(event(false, 1001), Some(now)));
            assert!(guard.key(event(true, 1002), Some(now)));
        }
    }

    #[test]
    fn shortcuts_reject_presses_before_visibility_or_start_and_allow_the_exact_boundary() {
        let now = Instant::now();
        let mut guard = SkipGuard {
            visible_at: None,
            held: [false; 3],
        };
        let event = |key, millis| KeyEvent {
            key,
            pressed: true,
            at: now + Duration::from_millis(millis),
        };
        assert!(!guard.key(event(0, 0), None));
        guard.visible_at = Some(now);
        assert!(!guard.key(event(1, 1100), Some(now + Duration::from_millis(1200))));
        assert!(guard.key(event(2, 1000), Some(now)));
    }

    #[test]
    fn keys_held_when_the_popup_opens_need_releasing() {
        let now = Instant::now();
        let mut guard = SkipGuard {
            visible_at: Some(now),
            held: [true; 3],
        };
        for key in 0..3 {
            let mut event = KeyEvent {
                key,
                pressed: true,
                at: now + Duration::from_secs(2),
            };
            assert!(!guard.key(event, Some(now)));
            event = KeyEvent {
                key,
                pressed: false,
                at: now + Duration::from_secs(2),
            };
            assert!(!guard.key(event, Some(now)));
            assert!(guard.key(
                KeyEvent {
                    key,
                    pressed: true,
                    at: now + Duration::from_secs(2)
                },
                Some(now)
            ));
        }
    }
}
