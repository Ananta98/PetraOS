// SPDX-License-Identifier: GPL-2.0

//! USB HID Keyboard Driver.
//!
//! Handles USB HID Boot Protocol keyboard reports, keycode translation,
//! modifier state decoding, DevFS character device registration (`/dev/usb_kbd`),
//! and reports key transitions to the input subsystem.
//!
//! The console is *not* fed from here: [`handle_usb_keyboard_report`] emits
//! `EV_KEY` events into the input core, and the console keyboard handler
//! (`drivers::char::tty::kbd`) turns those into terminal input. The `KeyCode`
//! and modifier conversion below therefore only serves the legacy
//! `/dev/usb_kbd` byte stream, exactly like the PS/2 driver keeps its own.

use alloc::{string::String, sync::Arc};
use spin::{Mutex, Once};

use crate::{
    drivers::{
        CharacterDevice,
        input::{self, InputDevice, keymap},
    },
    uapi::{
        errno::Result,
        input::{InputId, bus},
    },
    utils::ring_buffer::RingBuffer,
};

/// Interrupt-safe, lock-free ring buffer for USB keyboard characters.
static KEYBOARD_BUFFER: RingBuffer<1024> = RingBuffer::new();
static KEYBOARD_REGISTERED: Once<()> = Once::new();

/// The input device this keyboard reports to (`/dev/input/eventN`).
static INPUT_DEVICE: Once<Arc<InputDevice>> = Once::new();

/// HID modifier byte of the previous report, for edge detection.
static PREV_MODIFIERS: Mutex<u8> = Mutex::new(0);

pub struct UsbKeyboardDevice;

impl UsbKeyboardDevice {
    pub const fn new() -> Self {
        Self
    }
}

impl CharacterDevice for UsbKeyboardDevice {
    fn name(&self) -> String {
        "usb_kbd".into()
    }

    fn read(&self, buf: &mut [u8]) -> Result<usize> {
        let mut count = 0;
        while count < buf.len() {
            if let Some(byte) = KEYBOARD_BUFFER.pop() {
                buf[count] = byte;
                count += 1;
            } else {
                break;
            }
        }
        Ok(count)
    }

    fn write(&self, buf: &[u8]) -> Result<usize> {
        Ok(buf.len())
    }
}

pub fn init_usb_keyboard() -> Result<()> {
    KEYBOARD_REGISTERED.call_once(|| {
        let _ = crate::drivers::register_chrdev(Arc::new(UsbKeyboardDevice::new()));
    });
    INPUT_DEVICE.call_once(|| {
        input::register_device(
            "USB HID Keyboard",
            "usb-hid/keyboard",
            InputId {
                bustype: bus::USB,
                vendor: 0x0000,
                product: 0x0000,
                version: 0x0100,
            },
            keymap::keyboard_capabilities(),
        )
    });
    ostd::info!("USB Keyboard: Driver registered into unified character devices");
    Ok(())
}

/// Dispatches a decoded character or ANSI escape sequence to the legacy
/// `/dev/usb_kbd` byte stream.
pub fn handle_usb_keyboard_char(ch: char) {
    KEYBOARD_BUFFER.push_overwrite(ch as u8);
    ostd::debug!(
        "[USB-KEYBOARD] Key: '{}' ({:#04X})",
        ch.escape_default(),
        ch as u32
    );
}

/// Dispatches a raw string/sequence (such as escape sequences for arrow keys)
/// to the legacy `/dev/usb_kbd` byte stream.
pub fn handle_usb_keyboard_str(s: &str) {
    for b in s.bytes() {
        KEYBOARD_BUFFER.push_overwrite(b);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyCode {
    ErrorRollOver,
    POSTFail,
    ErrorUndefined,
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    Num1,
    Num2,
    Num3,
    Num4,
    Num5,
    Num6,
    Num7,
    Num8,
    Num9,
    Num0,
    Enter,
    Escape,
    Backspace,
    Tab,
    Space,
    Minus,
    Equal,
    LeftBrace,
    RightBrace,
    Backslash,
    HashTilde,
    Semicolon,
    Apostrophe,
    Grave,
    Comma,
    Dot,
    Slash,
    CapsLock,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    PrintScreen,
    ScrollLock,
    Pause,
    Insert,
    Home,
    PageUp,
    Delete,
    End,
    PageDown,
    RightArrow,
    LeftArrow,
    DownArrow,
    UpArrow,
    NumLock,
    KpSlash,
    KpAsterisk,
    KpMinus,
    KpPlus,
    KpEnter,
    Kp1,
    Kp2,
    Kp3,
    Kp4,
    Kp5,
    Kp6,
    Kp7,
    Kp8,
    Kp9,
    Kp0,
    KpDot,
    Unknown(u8),
}

impl KeyCode {
    /// Returns the base lowercase character if this key is alphabetic (A-Z).
    pub fn to_alpha_char(self) -> Option<char> {
        match self {
            KeyCode::A => Some('a'),
            KeyCode::B => Some('b'),
            KeyCode::C => Some('c'),
            KeyCode::D => Some('d'),
            KeyCode::E => Some('e'),
            KeyCode::F => Some('f'),
            KeyCode::G => Some('g'),
            KeyCode::H => Some('h'),
            KeyCode::I => Some('i'),
            KeyCode::J => Some('j'),
            KeyCode::K => Some('k'),
            KeyCode::L => Some('l'),
            KeyCode::M => Some('m'),
            KeyCode::N => Some('n'),
            KeyCode::O => Some('o'),
            KeyCode::P => Some('p'),
            KeyCode::Q => Some('q'),
            KeyCode::R => Some('r'),
            KeyCode::S => Some('s'),
            KeyCode::T => Some('t'),
            KeyCode::U => Some('u'),
            KeyCode::V => Some('v'),
            KeyCode::W => Some('w'),
            KeyCode::X => Some('x'),
            KeyCode::Y => Some('y'),
            KeyCode::Z => Some('z'),
            _ => None,
        }
    }
}

impl From<u8> for KeyCode {
    fn from(val: u8) -> Self {
        match val {
            0x01 => KeyCode::ErrorRollOver,
            0x02 => KeyCode::POSTFail,
            0x03 => KeyCode::ErrorUndefined,
            0x04 => KeyCode::A,
            0x05 => KeyCode::B,
            0x06 => KeyCode::C,
            0x07 => KeyCode::D,
            0x08 => KeyCode::E,
            0x09 => KeyCode::F,
            0x0A => KeyCode::G,
            0x0B => KeyCode::H,
            0x0C => KeyCode::I,
            0x0D => KeyCode::J,
            0x0E => KeyCode::K,
            0x0F => KeyCode::L,
            0x10 => KeyCode::M,
            0x11 => KeyCode::N,
            0x12 => KeyCode::O,
            0x13 => KeyCode::P,
            0x14 => KeyCode::Q,
            0x15 => KeyCode::R,
            0x16 => KeyCode::S,
            0x17 => KeyCode::T,
            0x18 => KeyCode::U,
            0x19 => KeyCode::V,
            0x1A => KeyCode::W,
            0x1B => KeyCode::X,
            0x1C => KeyCode::Y,
            0x1D => KeyCode::Z,
            0x1E => KeyCode::Num1,
            0x1F => KeyCode::Num2,
            0x20 => KeyCode::Num3,
            0x21 => KeyCode::Num4,
            0x22 => KeyCode::Num5,
            0x23 => KeyCode::Num6,
            0x24 => KeyCode::Num7,
            0x25 => KeyCode::Num8,
            0x26 => KeyCode::Num9,
            0x27 => KeyCode::Num0,
            0x28 => KeyCode::Enter,
            0x29 => KeyCode::Escape,
            0x2A => KeyCode::Backspace,
            0x2B => KeyCode::Tab,
            0x2C => KeyCode::Space,
            0x2D => KeyCode::Minus,
            0x2E => KeyCode::Equal,
            0x2F => KeyCode::LeftBrace,
            0x30 => KeyCode::RightBrace,
            0x31 => KeyCode::Backslash,
            0x32 => KeyCode::HashTilde,
            0x33 => KeyCode::Semicolon,
            0x34 => KeyCode::Apostrophe,
            0x35 => KeyCode::Grave,
            0x36 => KeyCode::Comma,
            0x37 => KeyCode::Dot,
            0x38 => KeyCode::Slash,
            0x39 => KeyCode::CapsLock,
            0x3A => KeyCode::F1,
            0x3B => KeyCode::F2,
            0x3C => KeyCode::F3,
            0x3D => KeyCode::F4,
            0x3E => KeyCode::F5,
            0x3F => KeyCode::F6,
            0x40 => KeyCode::F7,
            0x41 => KeyCode::F8,
            0x42 => KeyCode::F9,
            0x43 => KeyCode::F10,
            0x44 => KeyCode::F11,
            0x45 => KeyCode::F12,
            0x46 => KeyCode::PrintScreen,
            0x47 => KeyCode::ScrollLock,
            0x48 => KeyCode::Pause,
            0x49 => KeyCode::Insert,
            0x4A => KeyCode::Home,
            0x4B => KeyCode::PageUp,
            0x4C => KeyCode::Delete,
            0x4D => KeyCode::End,
            0x4E => KeyCode::PageDown,
            0x4F => KeyCode::RightArrow,
            0x50 => KeyCode::LeftArrow,
            0x51 => KeyCode::DownArrow,
            0x52 => KeyCode::UpArrow,
            0x53 => KeyCode::NumLock,
            0x54 => KeyCode::KpSlash,
            0x55 => KeyCode::KpAsterisk,
            0x56 => KeyCode::KpMinus,
            0x57 => KeyCode::KpPlus,
            0x58 => KeyCode::KpEnter,
            0x59 => KeyCode::Kp1,
            0x5A => KeyCode::Kp2,
            0x5B => KeyCode::Kp3,
            0x5C => KeyCode::Kp4,
            0x5D => KeyCode::Kp5,
            0x5E => KeyCode::Kp6,
            0x5F => KeyCode::Kp7,
            0x60 => KeyCode::Kp8,
            0x61 => KeyCode::Kp9,
            0x62 => KeyCode::Kp0,
            0x63 => KeyCode::KpDot,
            _ => KeyCode::Unknown(val),
        }
    }
}

pub struct Modifiers {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub gui: bool,
}

impl From<u8> for Modifiers {
    fn from(val: u8) -> Self {
        Modifiers {
            ctrl: (val & 0x11) != 0,
            shift: (val & 0x22) != 0,
            alt: (val & 0x44) != 0,
            gui: (val & 0x88) != 0,
        }
    }
}

static PREV_USB_KEYS: Mutex<[u8; 6]> = Mutex::new([0; 6]);
static CAPS_LOCK: Mutex<bool> = Mutex::new(false);
static NUM_LOCK: Mutex<bool> = Mutex::new(true);

pub fn handle_usb_keyboard_report(report: &[u8; 8]) {
    let modifiers = report[0];
    let current_keys: [u8; 6] = [
        report[2], report[3], report[4], report[5], report[6], report[7],
    ];
    let previous_keys = *PREV_USB_KEYS.lock();

    // Report the transition to the input core: this is the path evdev clients
    // and the console keyboard handler consume.
    if let Some(device) = INPUT_DEVICE.get() {
        report_key_events(device, modifiers, &previous_keys, &current_keys);
    }

    // Legacy `/dev/usb_kbd` byte stream, unchanged in behaviour.
    let mods = Modifiers::from(modifiers);
    let mut caps_lock = CAPS_LOCK.lock();
    let mut num_lock = NUM_LOCK.lock();

    for &raw_code in &current_keys {
        if raw_code == 0 || previous_keys.contains(&raw_code) {
            continue; // Empty or already pressed
        }

        let keycode = KeyCode::from(raw_code);

        // Ignore ghosting / rollover error codes
        if matches!(
            keycode,
            KeyCode::ErrorRollOver | KeyCode::POSTFail | KeyCode::ErrorUndefined
        ) {
            continue;
        }

        if keycode == KeyCode::CapsLock {
            *caps_lock = !*caps_lock;
            continue;
        }

        if keycode == KeyCode::NumLock {
            *num_lock = !*num_lock;
            continue;
        }

        // Check for navigation escape sequences (e.g. Arrow keys)
        if let Some(esc) = keycode_to_escape_seq(keycode) {
            handle_usb_keyboard_str(esc);
            continue;
        }

        if let Some(ch) = keycode_to_char(keycode, &mods, *caps_lock, *num_lock) {
            handle_usb_keyboard_char(ch);
        }
    }
    *PREV_USB_KEYS.lock() = current_keys;
}

/// Emits the `EV_KEY` edges of one report, then a single `SYN_REPORT`.
///
/// A boot keyboard report is a snapshot of the keys currently held, so the
/// edges are the difference between this and the previous report — both for the
/// modifier byte and for the six usage slots.
fn report_key_events(
    device: &InputDevice,
    modifiers: u8,
    previous_keys: &[u8; 6],
    current_keys: &[u8; 6],
) {
    let mut reported = false;

    {
        let mut previous_modifiers = PREV_MODIFIERS.lock();
        for (bit, code) in keymap::USB_MODIFIERS {
            let was = *previous_modifiers & bit != 0;
            let is = modifiers & bit != 0;
            if was != is {
                device.report_key(code, is as i32);
                reported = true;
            }
        }
        *previous_modifiers = modifiers;
    }

    for &usage in current_keys {
        // 0x00 is "no key"; 0x01..=0x03 are rollover/error codes.
        if usage < 0x04 || previous_keys.contains(&usage) {
            continue;
        }
        if let Some(code) = keymap::usb_usage(usage) {
            device.report_key(code, 1);
            reported = true;
        }
    }

    for &usage in previous_keys {
        if usage < 0x04 || current_keys.contains(&usage) {
            continue;
        }
        if let Some(code) = keymap::usb_usage(usage) {
            device.report_key(code, 0);
            reported = true;
        }
    }

    if reported {
        device.sync();
    }
}

fn keycode_to_escape_seq(key: KeyCode) -> Option<&'static str> {
    match key {
        KeyCode::UpArrow => Some("\x1B[A"),
        KeyCode::DownArrow => Some("\x1B[B"),
        KeyCode::RightArrow => Some("\x1B[C"),
        KeyCode::LeftArrow => Some("\x1B[D"),
        KeyCode::Home => Some("\x1B[H"),
        KeyCode::End => Some("\x1B[F"),
        KeyCode::PageUp => Some("\x1B[5~"),
        KeyCode::PageDown => Some("\x1B[6~"),
        KeyCode::Insert => Some("\x1B[2~"),
        KeyCode::Delete => Some("\x7F"),
        _ => None,
    }
}

fn keycode_to_char(
    key: KeyCode,
    mods: &Modifiers,
    caps_lock: bool,
    num_lock: bool,
) -> Option<char> {
    let shift = mods.shift;

    // Check if it's an alphabetic letter (A-Z)
    if let Some(base_char) = key.to_alpha_char() {
        if mods.ctrl {
            // Ctrl+A = \x01, Ctrl+Z = \x1A
            let ctrl_char = (base_char as u8 - b'a' + 1) as char;
            return Some(ctrl_char);
        }

        let is_upper = shift ^ caps_lock;
        if is_upper {
            return Some((base_char as u8 - 32) as char);
        }
        return Some(base_char);
    }

    match key {
        KeyCode::Num1 => Some(if shift { '!' } else { '1' }),
        KeyCode::Num2 => Some(if shift { '@' } else { '2' }),
        KeyCode::Num3 => Some(if shift { '#' } else { '3' }),
        KeyCode::Num4 => Some(if shift { '$' } else { '4' }),
        KeyCode::Num5 => Some(if shift { '%' } else { '5' }),
        KeyCode::Num6 => Some(if shift { '^' } else { '6' }),
        KeyCode::Num7 => Some(if shift { '&' } else { '7' }),
        KeyCode::Num8 => Some(if shift { '*' } else { '8' }),
        KeyCode::Num9 => Some(if shift { '(' } else { '9' }),
        KeyCode::Num0 => Some(if shift { ')' } else { '0' }),

        KeyCode::Enter => Some('\n'),
        KeyCode::Escape => Some('\x1B'),
        KeyCode::Backspace => Some(0x08 as char),
        KeyCode::Tab => Some('\t'),
        KeyCode::Space => Some(' '),

        KeyCode::Minus => Some(if shift { '_' } else { '-' }),
        KeyCode::Equal => Some(if shift { '+' } else { '=' }),
        KeyCode::LeftBrace => Some(if shift { '{' } else { '[' }),
        KeyCode::RightBrace => Some(if shift { '}' } else { ']' }),
        KeyCode::Backslash => Some(if shift { '|' } else { '\\' }),
        KeyCode::Semicolon => Some(if shift { ':' } else { ';' }),
        KeyCode::Apostrophe => Some(if shift { '"' } else { '\'' }),
        KeyCode::Grave => Some(if shift { '~' } else { '`' }),
        KeyCode::Comma => Some(if shift { '<' } else { ',' }),
        KeyCode::Dot => Some(if shift { '>' } else { '.' }),
        KeyCode::Slash => Some(if shift { '?' } else { '/' }),

        // Numeric Keypad
        KeyCode::KpSlash => Some('/'),
        KeyCode::KpAsterisk => Some('*'),
        KeyCode::KpMinus => Some('-'),
        KeyCode::KpPlus => Some('+'),
        KeyCode::KpEnter => Some('\n'),
        KeyCode::KpDot => Some(if num_lock { '.' } else { '\x7F' }),
        KeyCode::Kp1 if num_lock => Some('1'),
        KeyCode::Kp2 if num_lock => Some('2'),
        KeyCode::Kp3 if num_lock => Some('3'),
        KeyCode::Kp4 if num_lock => Some('4'),
        KeyCode::Kp5 if num_lock => Some('5'),
        KeyCode::Kp6 if num_lock => Some('6'),
        KeyCode::Kp7 if num_lock => Some('7'),
        KeyCode::Kp8 if num_lock => Some('8'),
        KeyCode::Kp9 if num_lock => Some('9'),
        KeyCode::Kp0 if num_lock => Some('0'),

        _ => None,
    }
}
