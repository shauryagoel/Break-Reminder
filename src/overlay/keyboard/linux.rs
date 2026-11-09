use std::{
    sync::mpsc::Sender,
    time::{Duration, Instant},
};

use eframe::egui;
use x11rb::{
    connection::Connection,
    protocol::{
        Event,
        xkb::{self, PerClientFlag},
        xproto::{
            Allow, AtomEnum, ConnectionExt, CreateWindowAux, EventMask, GrabMode, ModMask,
            PropMode, WindowClass,
        },
    },
    rust_connection::RustConnection,
};

use super::KeyEvent;

const KEY_SYMBOLS: [u32; 3] = [0xff0d, 0xff8d, 0xff1b];

pub(super) struct Keyboard {
    connection: RustConnection,
    keys: [u8; 3],
    held: [bool; 3],
    last_press: [Option<u16>; 3],
    locks: u16,
    server_time: u32,
    instant: Instant,
    sender: Sender<KeyEvent>,
    context: egui::Context,
}

impl Keyboard {
    pub(super) fn new(sender: Sender<KeyEvent>, context: egui::Context) -> Result<Self, String> {
        Self::connect(sender, context)
            .map_err(|error| format!("cannot reserve reminder skip keys: {error}"))
    }

    fn connect(
        sender: Sender<KeyEvent>,
        context: egui::Context,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let (connection, screen) = x11rb::connect(None)?;
        let root = connection.setup().roots[screen].root;
        if !xkb::use_extension(&connection, 1, 0)?.reply()?.supported {
            return Err("XKB keyboard extension is unavailable".into());
        }
        let repeat = xkb::per_client_flags(
            &connection,
            xkb::ID::USE_CORE_KBD.into(),
            PerClientFlag::DETECTABLE_AUTO_REPEAT,
            PerClientFlag::DETECTABLE_AUTO_REPEAT,
            0_u32.into(),
            0_u32.into(),
            0_u32.into(),
        )?
        .reply()?;
        if u32::from(repeat.value) & u32::from(PerClientFlag::DETECTABLE_AUTO_REPEAT) == 0 {
            return Err("X11 cannot distinguish held skip keys from fresh presses".into());
        }

        let setup = connection.setup();
        let minimum = setup.min_keycode;
        let count = setup.max_keycode - minimum + 1;
        let mapping = connection.get_keyboard_mapping(minimum, count)?.reply()?;
        let mut keys = [0; 3];
        for (index, wanted) in KEY_SYMBOLS.iter().enumerate() {
            let offset = mapping
                .keysyms
                .chunks(usize::from(mapping.keysyms_per_keycode))
                .position(|symbols| symbols.contains(wanted))
                .ok_or("X11 keyboard has no Return, keypad Enter, or Escape key")?;
            keys[index] = minimum + u8::try_from(offset)?;
        }
        let modifiers = connection.get_modifier_mapping()?.reply()?;
        let mut num_lock = 0_u16;
        for (modifier, codes) in modifiers
            .keycodes
            .chunks(usize::from(modifiers.keycodes_per_modifier()).max(1))
            .enumerate()
        {
            for code in codes.iter().filter(|code| **code >= minimum) {
                let start = usize::from(*code - minimum) * usize::from(mapping.keysyms_per_keycode);
                if mapping
                    .keysyms
                    .get(start..start + usize::from(mapping.keysyms_per_keycode))
                    .is_some_and(|symbols| symbols.contains(&0xff7f))
                {
                    num_lock |= 1 << modifier;
                }
            }
        }
        let caps_lock = u16::from(ModMask::LOCK);
        let mut masks = vec![0, caps_lock, num_lock, caps_lock | num_lock];
        masks.sort_unstable();
        masks.dedup();

        // A private property notification gives the same clock used by key events.
        let clock_window = connection.generate_id()?;
        connection
            .create_window(
                0,
                clock_window,
                root,
                0,
                0,
                1,
                1,
                0,
                WindowClass::INPUT_ONLY,
                0,
                &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
            )?
            .check()?;
        let instant = Instant::now();
        connection
            .change_property(
                PropMode::REPLACE,
                clock_window,
                AtomEnum::WM_NAME,
                AtomEnum::STRING,
                8,
                1,
                b"t",
            )?
            .check()?;
        let server_time = loop {
            if let Event::PropertyNotify(event) = connection.wait_for_event()? {
                break event.time;
            }
        };
        connection.destroy_window(clock_window)?.check()?;

        for key in keys {
            for mask in &masks {
                // Replay other keys if one arrives while a skip key owns the passive grab.
                connection
                    .grab_key(
                        false,
                        root,
                        (*mask).into(),
                        key,
                        GrabMode::ASYNC,
                        GrabMode::SYNC,
                    )?
                    .check()?;
            }
        }
        let state = connection.query_keymap()?.reply()?.keys;
        let held = keys.map(|key| state[usize::from(key / 8)] & (1 << (key % 8)) != 0);
        Ok(Self {
            connection,
            keys,
            held,
            last_press: [None; 3],
            locks: caps_lock | num_lock,
            server_time,
            instant,
            sender,
            context,
        })
    }

    pub(super) fn held(&self) -> [bool; 3] {
        self.held
    }

    pub(super) fn poll(&mut self) -> Result<(), String> {
        self.read_events()
            .map_err(|error| format!("cannot read reminder skip keys: {error}"))?;
        self.context
            .request_repaint_after(Duration::from_millis(20));
        Ok(())
    }

    fn read_events(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.drain_events()?;
        // A key held before registration, or after replay, may release outside our grab.
        if self.held.iter().any(|held| *held) {
            let sampled_at = Instant::now();
            let query = self.connection.query_keymap()?;
            let sequence = query.sequence_number() as u16;
            let state = query.reply()?.keys;
            self.drain_events()?;
            for key in 0..self.keys.len() {
                let code = self.keys[key];
                if self.held[key]
                    && self.last_press[key].is_none_or(|press| precedes(press, sequence))
                    && state[usize::from(code / 8)] & (1 << (code % 8)) == 0
                {
                    self.held[key] = false;
                    let _ = self.sender.send(KeyEvent {
                        key,
                        pressed: false,
                        at: sampled_at,
                    });
                }
            }
        }
        Ok(())
    }

    fn drain_events(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        while let Some(event) = self.connection.poll_for_event()? {
            let (event, pressed) = match event {
                Event::KeyPress(event) => (event, true),
                Event::KeyRelease(event) => (event, false),
                Event::Error(error) => return Err(format!("X11 server error: {error:?}").into()),
                _ => continue,
            };
            if u16::from(event.state) & 0xff & !self.locks == 0
                && let Some(key) = self.keys.iter().position(|code| *code == event.detail)
            {
                self.held[key] = pressed;
                let elapsed =
                    Duration::from_millis(u64::from(event.time.wrapping_sub(self.server_time)));
                let at = self.instant.checked_add(elapsed).unwrap_or(self.instant);
                if pressed {
                    self.last_press[key] = Some(event.sequence);
                }
                let _ = self.sender.send(KeyEvent { key, pressed, at });
                self.connection
                    .allow_events(Allow::SYNC_KEYBOARD, event.time)?;
            } else {
                self.connection
                    .allow_events(Allow::REPLAY_KEYBOARD, event.time)?;
            }
            self.connection.flush()?;
        }
        Ok(())
    }
}

fn precedes(event: u16, query: u16) -> bool {
    let age = query.wrapping_sub(event);
    age > 0 && age < 1 << 15
}

#[cfg(test)]
mod tests {
    use super::precedes;

    #[test]
    fn keymap_snapshot_does_not_release_a_press_arriving_after_the_query() {
        assert!(precedes(9, 10));
        assert!(!precedes(10, 10));
        assert!(!precedes(11, 10));
        assert!(precedes(u16::MAX, 0));
        assert!(!precedes(0, u16::MAX));
    }
}
