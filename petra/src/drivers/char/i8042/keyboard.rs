// SPDX-License-Identifier: GPL-2.0

//! i8042 PS/2 Keyboard Driver using `pc-keyboard`.
//!
//! Provides scancode decoding via the `pc-keyboard` crate for Scan Code Set 1,
//! US 104-key layout mapping, modifier tracking, ANSI escape sequence generation
//! for navigation keys, lock-free ring buffer queueing, and character device
//! registration for `/dev/keyboard`.

use alloc::{string::String, sync::Arc};
use ostd::{
    arch::{
        irq::{IRQ_CHIP, MappedIrqLine},
        trap::TrapFrame,
    },
    irq::IrqLine,
};
pub use pc_keyboard::{
    DecodedKey, HandleControl, KeyCode, KeyState, KeyState as KeyStatus, PS2Keyboard, ScancodeSet1,
    layouts::Us104Key,
};
use spin::{Mutex, Once};

use super::{
    controller::{I8042_CONTROLLER, I8042Controller},
    ps2::{CMD_ENABLE_SCANNING, CMD_RESET, send_keyboard_cmd},
};
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

const ISA_KEYBOARD_IRQ: u8 = 1;

/// IRQ line for i8042 keyboard.
static KEYBOARD_IRQ_LINE: Once<MappedIrqLine> = Once::new();

/// Global lock-free ring buffer storing received keyboard ASCII characters.
pub static KEYBOARD_BUFFER: RingBuffer<1024> = RingBuffer::new();

/// The input device this keyboard reports to (`/dev/input/eventN`).
static INPUT_DEVICE: Once<Arc<InputDevice>> = Once::new();

/// Global PS/2 keyboard state machine backed by `pc-keyboard`.
static KEYBOARD: Mutex<PS2Keyboard<Us104Key, ScancodeSet1>> = Mutex::new(PS2Keyboard::new(
    ScancodeSet1::new(),
    Us104Key,
    HandleControl::MapLettersToUnicode,
));

// ----------------------------------------------------------------------------
// Public Compatibility Types
// ----------------------------------------------------------------------------

/// How Scan Code Set 2 scancodes are translated to Set 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TranslationMode {
    /// The i8042 controller translates Set 2 to Set 1 in hardware.
    Hardware = 0,
    /// No hardware translation; the driver translates in software.
    Software = 1,
}

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
// Interrupt Handler & Hardware Integration
// ----------------------------------------------------------------------------

fn handle_keyboard_irq(_frame: &TrapFrame) {
    let Some(controller_lock) = I8042_CONTROLLER.get() else {
        return;
    };

    loop {
        let maybe_data = {
            let controller = controller_lock.lock();
            controller.receive_data()
        };

        let Some(data) = maybe_data else {
            break;
        };

        let mut keyboard = KEYBOARD.lock();
        let Ok(Some(key_event)) = keyboard.add_byte(data) else {
            continue;
        };

        // Report the key transition to the input core first: this is what
        // evdev clients and the console keyboard handler consume.
        if let Some(device) = INPUT_DEVICE.get() {
            report_key(device, &key_event);
        }

        // Legacy `/dev/keyboard` byte stream.
        if let Some(decoded) = keyboard.process_keyevent(key_event) {
            push_legacy_key(decoded);
        }
    }
}

fn push_legacy_key(decoded: DecodedKey) {
    match decoded {
        DecodedKey::Unicode('\n') => {
            // Return/Enter translates to carriage return \r for terminal line discipline
            KEYBOARD_BUFFER.push_overwrite(b'\r');
        }
        DecodedKey::Unicode(ch) => {
            let mut buf = [0u8; 4];
            for &b in ch.encode_utf8(&mut buf).as_bytes() {
                KEYBOARD_BUFFER.push_overwrite(b);
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

/// Emits one `EV_KEY` transition plus its `SYN_REPORT` into the input core.
fn report_key(device: &InputDevice, event: &pc_keyboard::KeyEvent) {
    let Some(code) = keymap::ps2_keycode(event.code) else {
        return;
    };
    let pressed = !matches!(event.state, KeyState::Up);
    device.report_key(code, pressed as i32);
    device.sync();
}

// ----------------------------------------------------------------------------
// Character Device (/dev/keyboard)
// ----------------------------------------------------------------------------

pub struct KeyboardDevice;

impl CharacterDevice for KeyboardDevice {
    fn name(&self) -> String {
        "keyboard".into()
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

/// Probes and initializes the PS/2 keyboard, binds IRQ 1, and registers the driver.
pub fn init(controller: &I8042Controller) -> Result<()> {
    ostd::info!("PS/2 Keyboard: Initializing (pc-keyboard)...");

    // 1. Send reset command (non-fatal if keyboard ignores it)
    if let Err(err) = send_keyboard_cmd(controller, CMD_RESET) {
        ostd::warn!("PS/2 Keyboard: Reset command error (ignored): {:?}", err);
    }
    // Flush any self-test response (e.g. 0xAA)
    controller.flush_buffer();

    // 2. Enable keyboard scanning
    if let Err(err) = send_keyboard_cmd(controller, CMD_ENABLE_SCANNING) {
        ostd::warn!("PS/2 Keyboard: Enable scanning error (ignored): {:?}", err);
    }

    // 3. Register the evdev device (`/dev/input/eventN`)
    let device = input::register_device(
        "AT Translated Set 2 keyboard",
        "isa0060/serio0/input0",
        InputId {
            bustype: bus::I8042,
            vendor: 0x0001,
            product: 0x0001,
            version: 0xab41,
        },
        keymap::keyboard_capabilities(),
    );
    INPUT_DEVICE.call_once(|| device);

    // 4. Allocate and map ISA IRQ 1
    if let Some(chip) = IRQ_CHIP.get() {
        if let Ok(irq_line) = IrqLine::alloc() {
            match chip.map_isa_pin_to(irq_line, ISA_KEYBOARD_IRQ) {
                Ok(mut mapped_line) => {
                    mapped_line.on_active(handle_keyboard_irq);
                    KEYBOARD_IRQ_LINE.call_once(|| mapped_line);
                    ostd::info!(
                        "PS/2 Keyboard: Mapped ISA IRQ {} successfully",
                        ISA_KEYBOARD_IRQ
                    );
                }
                Err(err) => {
                    ostd::warn!(
                        "PS/2 Keyboard: Failed to map ISA IRQ {}: {:?}",
                        ISA_KEYBOARD_IRQ,
                        err
                    );
                }
            }
        }
    }

    // 5. Register into unified driver registry
    crate::drivers::register_chrdev(Arc::new(KeyboardDevice))?;

    ostd::info!("PS/2 Keyboard: Driver registered successfully.");
    Ok(())
}
