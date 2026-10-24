// SPDX-License-Identifier: GPL-2.0

//! USB HID Keyboard Driver.
//!
//! Handles USB HID Boot Protocol keyboard reports using `pc-keyboard` for
//! scancode-to-keycode translation, modifier decoding, DevFS character device
//! registration (`/dev/usb_kbd`), and reporting key transitions to the input subsystem.

use alloc::{string::String, sync::Arc, vec::Vec};
use pc_keyboard::{
    DecodedKey, EventDecoder, HandleControl, KeyCode, KeyState, KeyEvent,
    UsbBootKeyboardReport, UsbKeyboard, layouts::Us104Key,
};
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

static USB_KEYBOARD: Mutex<Option<UsbKeyboard<Us104Key>>> = Mutex::new(None);
static EVENT_DECODER: Mutex<Option<EventDecoder<Us104Key>>> = Mutex::new(None);

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

/// Dispatches a raw string/sequence to the legacy `/dev/usb_kbd` byte stream.
pub fn handle_usb_keyboard_str(s: &str) {
    for b in s.bytes() {
        KEYBOARD_BUFFER.push_overwrite(b);
    }
}

fn raw_key_to_escape_seq(code: KeyCode) -> Option<&'static str> {
    match code {
        KeyCode::ArrowUp => Some("\x1B[A"),
        KeyCode::ArrowDown => Some("\x1B[B"),
        KeyCode::ArrowRight => Some("\x1B[C"),
        KeyCode::ArrowLeft => Some("\x1B[D"),
        KeyCode::Home => Some("\x1B[H"),
        KeyCode::End => Some("\x1B[F"),
        KeyCode::PageUp => Some("\x1B[5~"),
        KeyCode::PageDown => Some("\x1B[6~"),
        KeyCode::Insert => Some("\x1B[2~"),
        KeyCode::Delete => Some("\x7F"),
        _ => None,
    }
}

pub fn handle_usb_keyboard_report(report: &[u8; 8]) {
    let boot_report = UsbBootKeyboardReport {
        modifiers: report[0],
        keys: [
            report[2], report[3], report[4], report[5], report[6], report[7],
        ],
    };

    let mut kbd_guard = USB_KEYBOARD.lock();
    let kbd = kbd_guard.get_or_insert_with(|| {
        UsbKeyboard::new(Us104Key, HandleControl::MapLettersToUnicode)
    });
    let mut dec_guard = EVENT_DECODER.lock();
    let decoder = dec_guard.get_or_insert_with(|| {
        EventDecoder::new(Us104Key, HandleControl::MapLettersToUnicode)
    });

    // Collect events from pc-keyboard's USB HID scancode state machine
    let events: Vec<KeyEvent> = kbd.handle_report_raw(&boot_report).collect();

    for ev in events {
        // 1. Report transition to input subsystem (evdev)
        if let Some(device) = INPUT_DEVICE.get() {
            if let Some(code) = keymap::ps2_keycode(ev.code) {
                let pressed = !matches!(ev.state, KeyState::Up);
                device.report_key(code, pressed as i32);
                device.sync();
            }
        }

        // 2. Decode for legacy `/dev/usb_kbd` byte stream
        if let Some(decoded) = decoder.process_keyevent(ev) {
            match decoded {
                DecodedKey::Unicode(ch) => {
                    if ch == '\n' {
                        KEYBOARD_BUFFER.push_overwrite(b'\r');
                    } else {
                        let mut buf = [0u8; 4];
                        for &b in ch.encode_utf8(&mut buf).as_bytes() {
                            KEYBOARD_BUFFER.push_overwrite(b);
                        }
                    }
                }
                DecodedKey::RawKey(code) => {
                    if let Some(seq) = raw_key_to_escape_seq(code) {
                        handle_usb_keyboard_str(seq);
                    }
                }
            }
        }
    }
}
