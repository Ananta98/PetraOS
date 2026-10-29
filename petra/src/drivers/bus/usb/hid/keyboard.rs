// SPDX-License-Identifier: GPL-2.0

//! USB HID Keyboard Driver using `pc-keyboard`.
//!
//! Provides scancode decoding via the `pc-keyboard` crate for USB HID Boot Protocol,
//! US 104-key layout mapping, modifier tracking, ANSI escape sequence generation
//! for navigation keys, lock-free ring buffer queueing, and character device
//! registration for `/dev/usb_kbd`.

use alloc::{string::String, sync::Arc, vec::Vec};
pub use pc_keyboard::{
    DecodedKey, EventDecoder, HandleControl, KeyCode, KeyState, KeyState as KeyStatus, KeyEvent,
    UsbBootKeyboardReport, UsbKeyboard as UsbKeyboardDecoder, layouts::Us104Key,
};
use spin::{Mutex, Once};

use crate::{
    drivers::{
        CharacterDevice,
        char::tty,
        input::{self, InputDevice, keymap},
    },
    uapi::{
        errno::Result,
        input::{InputId, bus},
    },
    utils::ring_buffer::RingBuffer,
};

// ----------------------------------------------------------------------------
// State Machine & Buffers
// ----------------------------------------------------------------------------

/// Global lock-free ring buffer storing received keyboard ASCII characters.
pub static KEYBOARD_BUFFER: RingBuffer<1024> = RingBuffer::new();

/// The input device this keyboard reports to (`/dev/input/eventN`).
static INPUT_DEVICE: Once<Arc<InputDevice>> = Once::new();

static KEYBOARD_REGISTERED: Once<()> = Once::new();

/// Global USB HID keyboard state machine backed by `pc-keyboard`.
pub struct UsbKeyboard {
    engine: Mutex<Option<UsbKeyboardDecoder<Us104Key>>>,
    decoder: Mutex<Option<EventDecoder<Us104Key>>>,
}

impl UsbKeyboard {
    /// Creates a new uninitialized `UsbKeyboard` instance.
    pub const fn new() -> Self {
        Self {
            engine: Mutex::new(None),
            decoder: Mutex::new(None),
        }
    }

    /// Ingests a raw 8-byte USB Boot Protocol Keyboard Report, returning parsed KeyEvents.
    pub fn handle_report(&self, report: &[u8; 8]) -> Vec<KeyEvent> {
        let boot_report = UsbBootKeyboardReport {
            modifiers: report[0],
            keys: [
                report[2], report[3], report[4], report[5], report[6], report[7],
            ],
        };

        let mut engine_guard = self.engine.lock();
        let engine = engine_guard.get_or_insert_with(|| {
            UsbKeyboardDecoder::new(Us104Key, HandleControl::MapLettersToUnicode)
        });

        engine.handle_report_raw(&boot_report).collect()
    }

    /// Processes a key event through the layout decoder into a decoded key.
    pub fn process_keyevent(&self, event: KeyEvent) -> Option<DecodedKey> {
        let mut decoder_guard = self.decoder.lock();
        let decoder = decoder_guard.get_or_insert_with(|| {
            EventDecoder::new(Us104Key, HandleControl::MapLettersToUnicode)
        });

        decoder.process_keyevent(event)
    }
}

/// Global USB keyboard decoder state machine.
pub static KEYBOARD: UsbKeyboard = UsbKeyboard::new();

// ----------------------------------------------------------------------------
// ANSI Escape Sequence Mapping for Navigation Keys
// ----------------------------------------------------------------------------

fn raw_key_to_escape_seq(code: KeyCode) -> Option<&'static str> {
    match code {
        KeyCode::ArrowUp => Some("\x1B[A"),
        KeyCode::ArrowDown => Some("\x1B[B"),
        KeyCode::ArrowRight => Some("\x1B[C"),
        KeyCode::ArrowLeft => Some("\x1B[D"),
        KeyCode::Home => Some("\x1B[1~"),
        KeyCode::End => Some("\x1B[4~"),
        KeyCode::PageUp => Some("\x1B[5~"),
        KeyCode::PageDown => Some("\x1B[6~"),
        KeyCode::Insert => Some("\x1B[2~"),
        KeyCode::Delete => Some("\x1B[3~"),
        KeyCode::F1 => Some("\x1BOP"),
        KeyCode::F2 => Some("\x1BOQ"),
        KeyCode::F3 => Some("\x1BOR"),
        KeyCode::F4 => Some("\x1BOS"),
        KeyCode::F5 => Some("\x1B[15~"),
        KeyCode::F6 => Some("\x1B[17~"),
        KeyCode::F7 => Some("\x1B[18~"),
        KeyCode::F8 => Some("\x1B[19~"),
        KeyCode::F9 => Some("\x1B[20~"),
        KeyCode::F10 => Some("\x1B[21~"),
        KeyCode::F11 => Some("\x1B[23~"),
        KeyCode::F12 => Some("\x1B[24~"),
        _ => None,
    }
}

// ----------------------------------------------------------------------------
// Report Handling & Input Subsystem Integration
// ----------------------------------------------------------------------------

/// Handles an incoming 8-byte USB Boot Protocol report.
pub fn handle_usb_keyboard_report(report: &[u8; 8]) {
    let events = KEYBOARD.handle_report(report);

    for key_event in events {
        // Report the key transition to the input core first: this is what
        // evdev clients and the console keyboard handler consume.
        if let Some(device) = INPUT_DEVICE.get() {
            report_key(device, &key_event);
        }

        // Legacy `/dev/usb_kbd` byte stream.
        if let Some(decoded) = KEYBOARD.process_keyevent(key_event) {
            match decoded {
                DecodedKey::Unicode(ch) => {
                    // Return/Enter translates to carriage return \r for terminal line discipline
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
                        for b in seq.bytes() {
                            KEYBOARD_BUFFER.push_overwrite(b);
                        }
                    }
                }
            }
        }
    }
}

/// Dispatches a raw string/sequence to the legacy `/dev/usb_kbd` byte stream.
pub fn handle_usb_keyboard_str(s: &str) {
    for b in s.bytes() {
        KEYBOARD_BUFFER.push_overwrite(b);
    }
}

/// Emits one `EV_KEY` transition plus its `SYN_REPORT` into the input core.
fn report_key(device: &InputDevice, event: &KeyEvent) {
    let Some(code) = keymap::ps2_keycode(event.code) else {
        return;
    };
    let pressed = !matches!(event.state, KeyState::Up);
    device.report_key(code, pressed as i32);
    device.sync();
}

// ----------------------------------------------------------------------------
// Character Device (/dev/usb_kbd)
// ----------------------------------------------------------------------------

pub struct UsbKeyboardDevice;

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

    fn ioctl(&self, cmd: u32, arg: usize) -> Result<usize> {
        if let Some(cons) = tty::get_console() {
            cons.ioctl(cmd, arg)
        } else {
            crate::return_errno!(ENOTTY, "inappropriate ioctl for device")
        }
    }

    fn poll(&self) -> Result<u32> {
        if let Some(cons) = tty::get_console() {
            cons.poll()
        } else {
            Ok(0)
        }
    }
}

// ----------------------------------------------------------------------------
// Initialization & Registration
// ----------------------------------------------------------------------------

/// Probes and initializes the USB HID keyboard driver, registers the evdev device,
/// and registers the character device into `/dev/usb_kbd`.
pub fn init_usb_keyboard() -> Result<()> {
    ostd::info!("USB Keyboard: Initializing (pc-keyboard)...");

    // 1. Register the evdev device (`/dev/input/eventN`)
    let device = input::register_device(
        "USB HID Keyboard",
        "usb-hid/keyboard",
        InputId {
            bustype: bus::USB,
            vendor: 0x0000,
            product: 0x0000,
            version: 0x0100,
        },
        keymap::keyboard_capabilities(),
    );
    INPUT_DEVICE.call_once(|| device);

    // 2. Register into unified driver registry
    KEYBOARD_REGISTERED.call_once(|| {
        let _ = crate::drivers::register_chrdev(Arc::new(UsbKeyboardDevice));
    });

    ostd::info!("USB Keyboard: Driver registered successfully.");
    Ok(())
}
