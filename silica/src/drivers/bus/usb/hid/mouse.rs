// SPDX-License-Identifier: GPL-2.0

//! USB HID Mouse Driver.
//!
//! Handles USB HID Boot Protocol mouse reports, formats standard PS/2 mouse packets
//! for desktop and application compatibility, and registers the DevFS character device (`/dev/usb_mouse`).
//!
//! As with the keyboard, the console side is not fed from here: motion and
//! button transitions are reported as `EV_REL`/`EV_KEY` into the input core, so
//! `/dev/input/eventN` and any in-kernel listener see the same stream. The PS/2
//! packet encoding below only serves the legacy `/dev/usb_mouse` byte stream.

use alloc::{string::String, sync::Arc};
use core::sync::atomic::{AtomicU8, Ordering};
use spin::Once;

use crate::{
    drivers::{
        CharacterDevice,
        input::{self, InputDevice, keymap},
    },
    uapi::{
        errno::Result,
        input::{InputId, bus, key, rel},
    },
    utils::ring_buffer::RingBuffer,
};

/// Interrupt-safe, lock-free ring buffer for USB mouse packets.
static MOUSE_BUFFER: RingBuffer<2048> = RingBuffer::new();
static MOUSE_REGISTERED: Once<()> = Once::new();

/// The input device this mouse reports to (`/dev/input/eventN`).
static INPUT_DEVICE: Once<Arc<InputDevice>> = Once::new();

/// Buttons held down at the previous report, as [`MouseButtons`] bit flags.
static BUTTON_STATE: AtomicU8 = AtomicU8::new(0);

pub struct UsbMouseDevice;

impl UsbMouseDevice {
    pub const fn new() -> Self {
        Self
    }
}

impl CharacterDevice for UsbMouseDevice {
    fn name(&self) -> String {
        "usb_mouse".into()
    }

    fn read(&self, buf: &mut [u8]) -> Result<usize> {
        let mut count = 0;
        while count < buf.len() {
            if let Some(byte) = MOUSE_BUFFER.pop() {
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

pub fn init_usb_mouse() -> Result<()> {
    MOUSE_REGISTERED.call_once(|| {
        let _ = crate::drivers::register_chrdev(Arc::new(UsbMouseDevice::new()));
    });
    INPUT_DEVICE.call_once(|| {
        input::register_device(
            "USB HID Mouse",
            "usb-hid/mouse",
            InputId {
                bustype: bus::USB,
                vendor: 0x0000,
                product: 0x0000,
                version: 0x0100,
            },
            keymap::mouse_capabilities(
                &[
                    key::BTN_LEFT,
                    key::BTN_RIGHT,
                    key::BTN_MIDDLE,
                    key::BTN_SIDE,
                    key::BTN_EXTRA,
                ],
                true,
            ),
        )
    });
    ostd::info!("USB Mouse: Driver registered into unified character devices");
    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub struct MouseButtons {
    pub left: bool,
    pub right: bool,
    pub middle: bool,
    pub side1: bool,
    pub side2: bool,
}

impl From<u8> for MouseButtons {
    fn from(val: u8) -> Self {
        MouseButtons {
            left: (val & 0x01) != 0,
            right: (val & 0x02) != 0,
            middle: (val & 0x04) != 0,
            side1: (val & 0x08) != 0,
            side2: (val & 0x10) != 0,
        }
    }
}

/// `BTN_LEFT` bit in [`BUTTON_STATE`].
const BUTTON_LEFT: u8 = 1 << 0;
/// `BTN_RIGHT` bit in [`BUTTON_STATE`].
const BUTTON_RIGHT: u8 = 1 << 1;
/// `BTN_MIDDLE` bit in [`BUTTON_STATE`].
const BUTTON_MIDDLE: u8 = 1 << 2;
/// `BTN_SIDE` bit in [`BUTTON_STATE`].
const BUTTON_SIDE: u8 = 1 << 3;
/// `BTN_EXTRA` bit in [`BUTTON_STATE`].
const BUTTON_EXTRA: u8 = 1 << 4;

impl MouseButtons {
    /// Packs the buttons into [`BUTTON_STATE`] flags.
    fn bits(self) -> u8 {
        let mut bits = 0u8;
        if self.left {
            bits |= BUTTON_LEFT;
        }
        if self.right {
            bits |= BUTTON_RIGHT;
        }
        if self.middle {
            bits |= BUTTON_MIDDLE;
        }
        if self.side1 {
            bits |= BUTTON_SIDE;
        }
        if self.side2 {
            bits |= BUTTON_EXTRA;
        }
        bits
    }
}

/// Reports one boot-mouse report to the input core as `EV_REL`/`EV_KEY`.
///
/// USB reports Y positive-down, which is already the evdev `REL_Y`
/// convention, so no axis flip is needed here (unlike PS/2).
fn report_motion(device: &InputDevice, buttons: &MouseButtons, dx: i16, dy: i16, dz: i8) {
    let current = buttons.bits();
    let previous = BUTTON_STATE.swap(current, Ordering::Relaxed);
    let mut reported = false;

    for (bit, code) in [
        (BUTTON_LEFT, key::BTN_LEFT),
        (BUTTON_RIGHT, key::BTN_RIGHT),
        (BUTTON_MIDDLE, key::BTN_MIDDLE),
        (BUTTON_SIDE, key::BTN_SIDE),
        (BUTTON_EXTRA, key::BTN_EXTRA),
    ] {
        let was = previous & bit != 0;
        let is = current & bit != 0;
        if was != is {
            device.report_key(code, is as i32);
            reported = true;
        }
    }

    if dx != 0 {
        device.report_rel(rel::X, dx as i32);
        reported = true;
    }
    if dy != 0 {
        device.report_rel(rel::Y, dy as i32);
        reported = true;
    }
    if dz != 0 {
        device.report_rel(rel::WHEEL, dz as i32);
        reported = true;
    }

    if reported {
        device.sync();
    }
}

/// Ingests a raw USB Boot Mouse report (buttons, dx, dy, dz) and translates it into IntelliMouse Explorer PS/2 format (4-byte).
pub fn handle_usb_mouse_report(buttons_raw: u8, dx: i16, dy: i16, dz: i8) {
    let buttons = MouseButtons::from(buttons_raw);

    // USB Y is typically positive-down, PS/2 Y is positive-up. Invert Y.
    let dy_ps2 = -dy;

    // Clamp values to fit in 9-bit signed integers (-255 to 255)
    let dx_clamp = dx.clamp(-255, 255);
    let dy_clamp = dy_ps2.clamp(-255, 255);

    let mut b0 = 0x08; // bit 3 is always 1 (PS/2 packet sync bit)
    if buttons.left {
        b0 |= 0x01;
    }
    if buttons.right {
        b0 |= 0x02;
    }
    if buttons.middle {
        b0 |= 0x04;
    }

    if dx_clamp < 0 {
        b0 |= 0x10; // X sign bit
    }
    if dy_clamp < 0 {
        b0 |= 0x20; // Y sign bit
    }

    if dx > 255 || dx < -255 {
        b0 |= 0x40; // X overflow
    }
    if dy_ps2 > 255 || dy_ps2 < -255 {
        b0 |= 0x80; // Y overflow
    }

    let b1 = (dx_clamp & 0xFF) as u8;
    let b2 = (dy_clamp & 0xFF) as u8;

    // IntelliMouse Explorer 4th byte:
    // dz in lower 4 bits (sign-extended in 2's complement).
    // Button 4 (side1) in bit 4, Button 5 (side2) in bit 5.
    let mut b3 = (dz.clamp(-8, 7) & 0x0F) as u8;
    if buttons.side1 {
        b3 |= 0x10;
    }
    if buttons.side2 {
        b3 |= 0x20;
    }

    MOUSE_BUFFER.push_overwrite(b0);
    MOUSE_BUFFER.push_overwrite(b1);
    MOUSE_BUFFER.push_overwrite(b2);
    MOUSE_BUFFER.push_overwrite(b3);

    if let Some(device) = INPUT_DEVICE.get() {
        report_motion(device, &buttons, dx, dy, dz);
    }

    if buttons.left || buttons.right || buttons.middle || buttons.side1 || buttons.side2 {
        ostd::debug!(
            "[USB-MOUSE] Click: L={}, R={}, M={}, S1={}, S2={}, dx={}, dy={} (PS/2: {}), dz={}",
            buttons.left,
            buttons.right,
            buttons.middle,
            buttons.side1,
            buttons.side2,
            dx,
            dy,
            dy_ps2,
            dz
        );
    }
}
