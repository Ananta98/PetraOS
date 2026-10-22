// SPDX-License-Identifier: GPL-2.0

//! USB HID Mouse Driver.
//!
//! Handles USB HID Boot Protocol mouse reports, formats standard PS/2 mouse packets
//! for desktop and application compatibility, and registers the DevFS character device (`/dev/usb_mouse`).

use alloc::{string::String, sync::Arc};
use spin::Once;

use crate::{drivers::CharacterDevice, uapi::errno::Result, utils::ring_buffer::RingBuffer};

/// Interrupt-safe, lock-free ring buffer for USB mouse packets.
static MOUSE_BUFFER: RingBuffer<2048> = RingBuffer::new();
static MOUSE_REGISTERED: Once<()> = Once::new();

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
        crate::drivers::register_chrdev(Arc::new(UsbMouseDevice::new()));
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

    if buttons.left || buttons.right || buttons.middle || buttons.side1 || buttons.side2 {
        ostd::info!(
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
