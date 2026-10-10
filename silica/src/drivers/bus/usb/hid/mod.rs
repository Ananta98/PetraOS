// SPDX-License-Identifier: GPL-2.0

//! USB Human Interface Device (HID) Subsystem.
//!
//! Handles USB HID Boot Protocol report parsing, interface probing,
//! and dispatches decoded input events to the USB keyboard and mouse drivers.

pub mod keyboard;
pub mod mouse;

use super::{
    InterfaceDescriptor, USB_CLASS_HID, USB_HID_PROTOCOL_KEYBOARD, USB_HID_PROTOCOL_MOUSE,
};
use crate::api::errno::{Errno, Result};

/// USB HID Device Type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HidDeviceType {
    Keyboard,
    Mouse,
    Generic,
}

/// Standard 8-byte USB Boot Protocol Keyboard Report.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct BootKeyboardReport {
    pub modifiers: u8,
    pub reserved: u8,
    pub keys: [u8; 6],
}

/// Standard USB Boot Protocol Mouse Report (3 or 4 bytes).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct BootMouseReport {
    pub buttons: u8,
    pub dx: i8,
    pub dy: i8,
    pub dz: i8,
}

/// Probes a USB Interface Descriptor to see if it is an HID device,
/// and delegates initialization to the USB HID drivers.
pub fn probe_hid_interface(iface: &InterfaceDescriptor) -> Result<HidDeviceType> {
    if iface.interface_class != USB_CLASS_HID {
        return Err(Errno::ENODEV);
    }

    match iface.interface_protocol {
        USB_HID_PROTOCOL_KEYBOARD => {
            ostd::info!("USB HID: Probing and initializing USB Keyboard...");
            keyboard::init_usb_keyboard()?;
            Ok(HidDeviceType::Keyboard)
        }
        USB_HID_PROTOCOL_MOUSE => {
            ostd::info!("USB HID: Probing and initializing USB Mouse...");
            mouse::init_usb_mouse()?;
            Ok(HidDeviceType::Mouse)
        }
        _ => {
            ostd::info!(
                "USB HID: Found generic HID interface (protocol: 0x{:02X})",
                iface.interface_protocol
            );
            Ok(HidDeviceType::Generic)
        }
    }
}

/// Dispatches raw incoming USB interrupt transfer data for a Keyboard.
pub fn dispatch_keyboard_data(data: &[u8]) {
    if data.len() < 8 {
        return;
    }
    let mut report = [0u8; 8];
    report.copy_from_slice(&data[..8]);
    keyboard::handle_usb_keyboard_report(&report);
}

/// Dispatches raw incoming USB interrupt transfer data for a Mouse.
pub fn dispatch_mouse_data(data: &[u8]) {
    if data.len() < 3 {
        return;
    }
    let buttons = data[0];
    let dx = data[1] as i8 as i16;
    let dy = data[2] as i8 as i16;
    let dz = if data.len() >= 4 { data[3] as i8 } else { 0 };

    mouse::handle_usb_mouse_report(buttons, dx, dy, dz);
}
