// SPDX-License-Identifier: GPL-2.0

//! Scancode-to-keycode tables and keycode-to-input translation.
//!
//! Two conversions live here, both shared by every keyboard driver:
//!
//! * **hardware → `KEY_*`** — [`ps2_keycode`] for PS/2 Scan Code Set 1 (as
//!   decoded by `pc-keyboard`) and [`usb_usage`] for USB HID usage IDs. This is
//!   what reaches evdev clients.
//! * **`KEY_*` → terminal input** — [`translate`], the US layout translation the
//!   TTY keyboard handler uses. Keeping it here rather than in the drivers
//!   means the console behaves identically for PS/2 and USB keyboards.

use crate::uapi::input::{key, rel};

use super::Capabilities;

/// The terminal bytes a key produces (escape sequence) or the character.
pub enum KeyOutput {
    /// A single character. Multi-byte UTF-8 is expanded by the caller.
    Char(char),
    /// A pre-encoded terminal escape sequence.
    Bytes(&'static [u8]),
}

/// Modifier and lock state tracked by the TTY keyboard handler.
#[derive(Clone, Copy, Debug, Default)]
pub struct KeyMods {
    pub lshift: bool,
    pub rshift: bool,
    pub lctrl: bool,
    pub rctrl: bool,
    pub lalt: bool,
    pub ralt: bool,
    pub lmeta: bool,
    pub rmeta: bool,
    pub caps: bool,
    pub numlock: bool,
}

impl KeyMods {
    pub fn shift(&self) -> bool {
        self.lshift || self.rshift
    }

    pub fn ctrl(&self) -> bool {
        self.lctrl || self.rctrl
    }

    /// Updates modifier/lock state from a key transition.
    ///
    /// Returns `true` when `code` is a modifier or lock key, so the caller does
    /// not also translate it into a character.
    pub fn update(&mut self, code: u16, pressed: bool) -> bool {
        let toggle = |state: &mut bool, changed: bool| {
            if changed {
                *state = !*state;
            }
        };
        match code {
            key::LEFTSHIFT => self.lshift = pressed,
            key::RIGHTSHIFT => self.rshift = pressed,
            key::LEFTCTRL => self.lctrl = pressed,
            key::RIGHTCTRL => self.rctrl = pressed,
            key::LEFTALT => self.lalt = pressed,
            key::RIGHTALT => self.ralt = pressed,
            key::LEFTMETA => self.lmeta = pressed,
            key::RIGHTMETA => self.rmeta = pressed,
            key::CAPSLOCK => toggle(&mut self.caps, pressed),
            key::NUMLOCK => toggle(&mut self.numlock, pressed),
            _ => return false,
        }
        true
    }
}

// ---------------------------------------------------------------------------
// PS/2 Scan Code Set 1 → KEY_*
// ---------------------------------------------------------------------------

/// Maps a `pc-keyboard` key to its Linux `KEY_*` code.
pub fn ps2_keycode(code: pc_keyboard::KeyCode) -> Option<u16> {
    use pc_keyboard::KeyCode as P;
    Some(match code {
        P::Escape => key::ESC,
        P::F1 => key::F1,
        P::F2 => key::F2,
        P::F3 => key::F3,
        P::F4 => key::F4,
        P::F5 => key::F5,
        P::F6 => key::F6,
        P::F7 => key::F7,
        P::F8 => key::F8,
        P::F9 => key::F9,
        P::F10 => key::F10,
        P::F11 => key::F11,
        P::F12 => key::F12,
        P::PrintScreen => key::SYSRQ,
        P::SysRq => key::SYSRQ,
        P::ScrollLock => key::SCROLLLOCK,
        P::PauseBreak => key::PAUSE,
        P::Oem8 => key::GRAVE,
        P::Key1 => key::NUM1,
        P::Key2 => key::NUM2,
        P::Key3 => key::NUM3,
        P::Key4 => key::NUM4,
        P::Key5 => key::NUM5,
        P::Key6 => key::NUM6,
        P::Key7 => key::NUM7,
        P::Key8 => key::NUM8,
        P::Key9 => key::NUM9,
        P::Key0 => key::NUM0,
        P::OemMinus => key::MINUS,
        P::OemPlus => key::EQUAL,
        P::Backspace => key::BACKSPACE,
        P::Tab => key::TAB,
        P::Q => key::Q,
        P::W => key::W,
        P::E => key::E,
        P::R => key::R,
        P::T => key::T,
        P::Y => key::Y,
        P::U => key::U,
        P::I => key::I,
        P::O => key::O,
        P::P => key::P,
        P::Oem4 => key::LEFTBRACE,
        P::Oem6 => key::RIGHTBRACE,
        P::Oem5 => key::BACKSLASH,
        P::Return => key::ENTER,
        P::A => key::A,
        P::S => key::S,
        P::D => key::D,
        P::F => key::F,
        P::G => key::G,
        P::H => key::H,
        P::J => key::J,
        P::K => key::K,
        P::L => key::L,
        P::Oem1 => key::SEMICOLON,
        P::Oem3 => key::APOSTROPHE,
        P::Oem7 => key::BACKSLASH,
        P::LShift => key::LEFTSHIFT,
        P::Z => key::Z,
        P::X => key::X,
        P::C => key::C,
        P::V => key::V,
        P::B => key::B,
        P::N => key::N,
        P::M => key::M,
        P::OemComma => key::COMMA,
        P::OemPeriod => key::DOT,
        P::Oem2 => key::SLASH,
        P::RShift => key::RIGHTSHIFT,
        P::LControl => key::LEFTCTRL,
        P::RControl | P::RControl2 => key::RIGHTCTRL,
        P::LAlt => key::LEFTALT,
        P::RAltGr | P::RAlt2 => key::RIGHTALT,
        P::LWin => key::LEFTMETA,
        P::RWin => key::RIGHTMETA,
        P::Apps => key::MENU,
        P::Spacebar => key::SPACE,
        P::CapsLock => key::CAPSLOCK,
        P::NumpadLock => key::NUMLOCK,
        P::NumpadDivide => key::KPSLASH,
        P::NumpadMultiply => key::KPASTERISK,
        P::NumpadSubtract => key::KPMINUS,
        P::NumpadAdd => key::KPPLUS,
        P::NumpadEnter => key::KPENTER,
        P::Numpad0 => key::KP0,
        P::Numpad1 => key::KP1,
        P::Numpad2 => key::KP2,
        P::Numpad3 => key::KP3,
        P::Numpad4 => key::KP4,
        P::Numpad5 => key::KP5,
        P::Numpad6 => key::KP6,
        P::Numpad7 => key::KP7,
        P::Numpad8 => key::KP8,
        P::Numpad9 => key::KP9,
        P::NumpadPeriod => key::KPDOT,
        P::Insert => key::INSERT,
        P::Delete => key::DELETE,
        P::Home => key::HOME,
        P::End => key::END,
        P::PageUp => key::PAGEUP,
        P::PageDown => key::PAGEDOWN,
        P::ArrowUp => key::UP,
        P::ArrowDown => key::DOWN,
        P::ArrowLeft => key::LEFT,
        P::ArrowRight => key::RIGHT,
        P::Oem9 => key::MUHENKAN,
        P::Oem10 => key::HENKAN,
        P::Oem11 => key::HIRAGANA,
        P::Oem12 => key::RO,
        P::Oem13 => key::YEN,
        P::PrevTrack => key::PREVIOUSSONG,
        P::NextTrack => key::NEXTSONG,
        P::Mute => key::MUTE,
        P::Play => key::PLAYPAUSE,
        P::Stop => key::STOPCD,
        P::VolumeDown => key::VOLUMEDOWN,
        P::VolumeUp => key::VOLUMEUP,
        P::Calculator => key::CALC,
        P::WWWHome => key::HOMEPAGE,
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// USB HID usage → KEY_*
// ---------------------------------------------------------------------------

/// The eight HID modifier bits of a boot keyboard report, with their codes.
pub const USB_MODIFIERS: [(u8, u16); 8] = [
    (1 << 0, key::LEFTCTRL),
    (1 << 1, key::LEFTSHIFT),
    (1 << 2, key::LEFTALT),
    (1 << 3, key::LEFTMETA),
    (1 << 4, key::RIGHTCTRL),
    (1 << 5, key::RIGHTSHIFT),
    (1 << 6, key::RIGHTALT),
    (1 << 7, key::RIGHTMETA),
];

/// Maps a USB HID keyboard usage ID (HID Usage Tables §10) to `KEY_*`.
pub fn usb_usage(usage: u8) -> Option<u16> {
    Some(match usage {
        // Letters are not contiguous in the `KEY_*` namespace: the codes follow
        // the PS/2 scancode order, so each usage is mapped explicitly.
        0x04 => key::A,
        0x05 => key::B,
        0x06 => key::C,
        0x07 => key::D,
        0x08 => key::E,
        0x09 => key::F,
        0x0a => key::G,
        0x0b => key::H,
        0x0c => key::I,
        0x0d => key::J,
        0x0e => key::K,
        0x0f => key::L,
        0x10 => key::M,
        0x11 => key::N,
        0x12 => key::O,
        0x13 => key::P,
        0x14 => key::Q,
        0x15 => key::R,
        0x16 => key::S,
        0x17 => key::T,
        0x18 => key::U,
        0x19 => key::V,
        0x1a => key::W,
        0x1b => key::X,
        0x1c => key::Y,
        0x1d => key::Z,
        0x1e..=0x26 => key::NUM1 + (usage - 0x1e) as u16,
        0x27 => key::NUM0,
        0x28 => key::ENTER,
        0x29 => key::ESC,
        0x2a => key::BACKSPACE,
        0x2b => key::TAB,
        0x2c => key::SPACE,
        0x2d => key::MINUS,
        0x2e => key::EQUAL,
        0x2f => key::LEFTBRACE,
        0x30 => key::RIGHTBRACE,
        0x31 => key::BACKSLASH,
        0x32 => key::KEY102ND,
        0x33 => key::SEMICOLON,
        0x34 => key::APOSTROPHE,
        0x35 => key::GRAVE,
        0x36 => key::COMMA,
        0x37 => key::DOT,
        0x38 => key::SLASH,
        0x39 => key::CAPSLOCK,
        0x3a..=0x43 => key::F1 + (usage - 0x3a) as u16,
        0x44 => key::F11,
        0x45 => key::F12,
        0x46 => key::SYSRQ,
        0x47 => key::SCROLLLOCK,
        0x48 => key::PAUSE,
        0x49 => key::INSERT,
        0x4a => key::HOME,
        0x4b => key::PAGEUP,
        0x4c => key::DELETE,
        0x4d => key::END,
        0x4e => key::PAGEDOWN,
        0x4f => key::RIGHT,
        0x50 => key::LEFT,
        0x51 => key::DOWN,
        0x52 => key::UP,
        0x53 => key::NUMLOCK,
        0x54 => key::KPSLASH,
        0x55 => key::KPASTERISK,
        0x56 => key::KPMINUS,
        0x57 => key::KPPLUS,
        0x58 => key::KPENTER,
        0x59 => key::KP1,
        0x5a => key::KP2,
        0x5b => key::KP3,
        0x5c => key::KP4,
        0x5d => key::KP5,
        0x5e => key::KP6,
        0x5f => key::KP7,
        0x60 => key::KP8,
        0x61 => key::KP9,
        0x62 => key::KP0,
        0x63 => key::KPDOT,
        0x64 => key::KEY102ND,
        0x65 => key::COMPOSE,
        0x66 => key::POWER,
        0x67 => key::KPEQUAL,
        0x68..=0x73 => key::F13 + (usage - 0x68) as u16,
        0x75 => key::HELP,
        0x76 => key::MENU,
        0x78 => key::STOP,
        0x79 => key::AGAIN,
        0x7a => key::UNDO,
        0x7b => key::CUT,
        0x7c => key::COPY,
        0x7d => key::PASTE,
        0x7e => key::FIND,
        0x7f => key::MUTE,
        0x80 => key::VOLUMEUP,
        0x81 => key::VOLUMEDOWN,
        0x85 => key::KPCOMMA,
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// KEY_* → terminal input (US layout)
// ---------------------------------------------------------------------------

/// Translates a key press into terminal input, or `None` if the key produces
/// no bytes (modifiers, locks, unhandled keys).
pub fn translate(code: u16, mods: &KeyMods) -> Option<KeyOutput> {
    let shift = mods.shift();

    // Letters: `Ctrl+A` is `0x01`, otherwise case is shift XOR capslock.
    if let Some(index) = letter_index(code) {
        if mods.ctrl() {
            return Some(KeyOutput::Char((index + 1) as char));
        }
        let lower = b'a' + index;
        let ch = if shift ^ mods.caps {
            lower.to_ascii_uppercase()
        } else {
            lower
        };
        return Some(KeyOutput::Char(ch as char));
    }

    // Number row.
    if (key::NUM1..=key::NUM0).contains(&code) {
        const PLAIN: [u8; 10] = *b"1234567890";
        const SHIFTED: [u8; 10] = *b"!@#$%^&*()";
        let index = (code - key::NUM1) as usize % 10;
        let byte = if shift { SHIFTED[index] } else { PLAIN[index] };
        return Some(KeyOutput::Char(byte as char));
    }

    let ch = match code {
        key::MINUS => shifted(shift, '-', '_'),
        key::EQUAL => shifted(shift, '=', '+'),
        key::LEFTBRACE => shifted(shift, '[', '{'),
        key::RIGHTBRACE => shifted(shift, ']', '}'),
        key::BACKSLASH => shifted(shift, '\\', '|'),
        key::SEMICOLON => shifted(shift, ';', ':'),
        key::APOSTROPHE => shifted(shift, '\'', '"'),
        key::GRAVE => shifted(shift, '`', '~'),
        key::COMMA => shifted(shift, ',', '<'),
        key::DOT => shifted(shift, '.', '>'),
        key::SLASH => shifted(shift, '/', '?'),
        key::SPACE => ' ',
        key::ENTER | key::KPENTER => '\r',
        key::TAB => '\t',
        key::ESC => '\x1b',
        key::BACKSPACE => '\x7f',
        key::KPSLASH => '/',
        key::KPASTERISK => '*',
        key::KPMINUS => '-',
        key::KPPLUS => '+',
        _ => {
            // Keypad digits need Num Lock; without it the keypad navigates.
            if mods.numlock {
                if let Some(digit) = keypad_digit(code) {
                    return Some(KeyOutput::Char(digit));
                }
            }
            return navigation(code).map(KeyOutput::Bytes);
        }
    };
    Some(KeyOutput::Char(ch))
}

/// The digit a numeric keypad key produces with Num Lock on.
fn keypad_digit(code: u16) -> Option<char> {
    Some(match code {
        key::KP0 => '0',
        key::KP1 => '1',
        key::KP2 => '2',
        key::KP3 => '3',
        key::KP4 => '4',
        key::KP5 => '5',
        key::KP6 => '6',
        key::KP7 => '7',
        key::KP8 => '8',
        key::KP9 => '9',
        key::KPDOT => '.',
        _ => return None,
    })
}

/// Position of `code` in the alphabet, from its `KEY_*` letter code.
///
/// The letter codes follow the PS/2 scancode order (`KEY_Q` is 16 but `KEY_B`
/// is 48), so this is a table rather than an offset.
fn letter_index(code: u16) -> Option<u8> {
    const LETTERS: [(u16, u8); 26] = [
        (key::A, 0),
        (key::B, 1),
        (key::C, 2),
        (key::D, 3),
        (key::E, 4),
        (key::F, 5),
        (key::G, 6),
        (key::H, 7),
        (key::I, 8),
        (key::J, 9),
        (key::K, 10),
        (key::L, 11),
        (key::M, 12),
        (key::N, 13),
        (key::O, 14),
        (key::P, 15),
        (key::Q, 16),
        (key::R, 17),
        (key::S, 18),
        (key::T, 19),
        (key::U, 20),
        (key::V, 21),
        (key::W, 22),
        (key::X, 23),
        (key::Y, 24),
        (key::Z, 25),
    ];
    LETTERS
        .iter()
        .find_map(|&(c, index)| (c == code).then_some(index))
}

fn shifted(shift: bool, plain: char, with_shift: char) -> char {
    if shift {
        with_shift
    } else {
        plain
    }
}

/// Escape sequences for navigation and function keys.
fn navigation(code: u16) -> Option<&'static [u8]> {
    Some(match code {
        key::UP | key::KP8 => b"\x1b[A",
        key::DOWN | key::KP2 => b"\x1b[B",
        key::RIGHT | key::KP6 => b"\x1b[C",
        key::LEFT | key::KP4 => b"\x1b[D",
        key::HOME | key::KP7 => b"\x1b[1~",
        key::END | key::KP1 => b"\x1b[4~",
        key::PAGEUP | key::KP9 => b"\x1b[5~",
        key::PAGEDOWN | key::KP3 => b"\x1b[6~",
        key::INSERT | key::KP0 => b"\x1b[2~",
        key::DELETE | key::KPDOT => b"\x1b[3~",
        key::F1 => b"\x1bOP",
        key::F2 => b"\x1bOQ",
        key::F3 => b"\x1bOR",
        key::F4 => b"\x1bOS",
        key::F5 => b"\x1b[15~",
        key::F6 => b"\x1b[17~",
        key::F7 => b"\x1b[18~",
        key::F8 => b"\x1b[19~",
        key::F9 => b"\x1b[20~",
        key::F10 => b"\x1b[21~",
        key::F11 => b"\x1b[23~",
        key::F12 => b"\x1b[24~",
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Device capability sets
// ---------------------------------------------------------------------------

/// The capability set of a 104-key keyboard.
///
/// Both the PS/2 and the USB driver advertise this, so userspace sees the same
/// key bitmap regardless of the transport.
pub fn keyboard_capabilities() -> Capabilities {
    use crate::uapi::input::ev;

    let mut caps = Capabilities::new();
    caps.set_ev(ev::KEY);
    caps.set_ev(ev::REP);
    // Esc, digits, letters, punctuation, modifiers, F1..F10 and the keypad
    // live in one contiguous block of the key namespace.
    for code in 1..=88 {
        caps.set_key(code);
    }
    // International, keypad Enter, navigation cluster and media keys.
    for code in 96..=100 {
        caps.set_key(code);
    }
    for code in 102..=111 {
        caps.set_key(code);
    }
    for code in [113, 114, 115, 116, 119, 125, 126, 127] {
        caps.set_key(code);
    }
    caps
}

/// The capability set of a mouse reporting `buttons` and optional wheel axes.
pub fn mouse_capabilities(buttons: &[u16], wheel: bool) -> Capabilities {
    use crate::uapi::input::ev;

    let mut caps = Capabilities::new();
    caps.set_ev(ev::KEY);
    caps.set_ev(ev::REL);
    for &button in buttons {
        caps.set_key(button);
    }
    caps.set_rel(rel::X);
    caps.set_rel(rel::Y);
    if wheel {
        caps.set_rel(rel::WHEEL);
        caps.set_rel(rel::HWHEEL);
    }
    caps
}

