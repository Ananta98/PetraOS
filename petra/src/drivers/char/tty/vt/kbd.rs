// SPDX-License-Identifier: GPL-2.0

//! Console keyboard handler: input subsystem → TTY line discipline.
//!
//! This is the in-kernel counterpart of Linux' `kbd` input handler. It listens
//! to every input device that reports `EV_KEY`, translates key transitions into
//! terminal input with the US layout in [`crate::drivers::input::keymap`], and
//! pushes the resulting bytes into the console queue.
//!
//! Drivers no longer feed the TTY themselves: PS/2 and USB keyboards both emit
//! `EV_KEY` events into the input core, and this handler is the single place
//! that decides what a keystroke means for the console. That keeps evdev
//! clients and the terminal seeing exactly the same key stream.

use ostd::sync::{LocalIrqDisabled, SpinLock};

use crate::{
    drivers::{
        char::tty,
        input::{
            InputDevice, InputListener,
            keymap::{self, KeyMods, KeyOutput},
        },
    },
    uapi::input::{InputEvent, ev, key, led},
};

/// The single console keyboard listener.
struct ConsoleKeyboard;

static CONSOLE_KEYBOARD: ConsoleKeyboard = ConsoleKeyboard;

/// Modifier and lock state shared by all keyboards attached to the console.
static MODIFIERS: SpinLock<KeyMods, LocalIrqDisabled> = SpinLock::new(KeyMods {
    // Linux boots with Num Lock on; keep the keypad typing digits by default.
    numlock: true,
    lshift: false,
    rshift: false,
    lctrl: false,
    rctrl: false,
    lalt: false,
    ralt: false,
    lmeta: false,
    rmeta: false,
    caps: false,
});

impl InputListener for ConsoleKeyboard {
    fn event(&self, device: &InputDevice, event: &InputEvent) {
        if event.type_ != ev::KEY {
            return;
        }
        // Button codes (BTN_*) belong to pointers, not to the console.
        if event.code >= key::BTN_MISC {
            return;
        }

        let pressed = event.value != 0;
        let (output, led_update) = {
            let mut mods = MODIFIERS.lock();
            if mods.update(event.code, pressed) {
                let led = if pressed {
                    match event.code {
                        key::CAPSLOCK => Some((led::CAPSL, mods.caps)),
                        key::NUMLOCK => Some((led::NUML, mods.numlock)),
                        _ => None,
                    }
                } else {
                    None
                };
                (None, led)
            } else if pressed {
                (keymap::translate(event.code, &mods), None)
            } else {
                (None, None)
            }
        };

        // Reporting outside the modifier lock lets the LED event come back
        // through this listener without re-entering it.
        if let Some((code, on)) = led_update {
            device.report_led(code, on as i32);
        }

        match output {
            Some(KeyOutput::Char(ch)) => {
                let mut utf8 = [0u8; 4];
                let s = ch.encode_utf8(&mut utf8);
                tty::queue_input_bytes(s.as_bytes());
            }
            Some(KeyOutput::Bytes(seq)) => {
                tty::queue_input_bytes(seq);
            }
            None => {}
        }
    }
}

/// Attaches the console keyboard handler to the input core.
///
/// Called from TTY initialisation; listeners registered later simply start
/// receiving subsequent events, so the order relative to the drivers does not
/// matter.
pub fn init() {
    crate::drivers::input::register_listener(&CONSOLE_KEYBOARD);
}
