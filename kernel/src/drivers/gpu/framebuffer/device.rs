//! Framebuffer and Console Device Definitions
//!
//! Exposes GPU framebuffer and character console devices to DEVICE_MANAGER.

use crate::device::{CharDevice, Device, DeviceType, DriverError, Major, Minor};
use super::console::fb_console_write_byte;
use super::fb::FRAMEBUFFER;

/// GPU Linear Framebuffer device.
pub struct FramebufferDevice;

impl Device for FramebufferDevice {
    fn major(&self) -> Major {
        29 // Linux FB_MAJOR
    }

    fn minor(&self) -> Minor {
        0
    }

    fn dev_type(&self) -> DeviceType {
        DeviceType::Gpu
    }

    fn name(&self) -> &'static str {
        "Limine Framebuffer"
    }

    fn init(&mut self) -> Result<(), DriverError> {
        if FRAMEBUFFER.lock().is_some() {
            Ok(())
        } else {
            Err(DriverError::InitFailed)
        }
    }
}

/// Character console device backed by the framebuffer.
pub struct FramebufferConsoleDevice;

impl Device for FramebufferConsoleDevice {
    fn major(&self) -> Major {
        4 // TTY_MAJOR
    }

    fn minor(&self) -> Minor {
        0
    }

    fn dev_type(&self) -> DeviceType {
        DeviceType::Char
    }

    fn name(&self) -> &'static str {
        "Framebuffer Console"
    }

    fn init(&mut self) -> Result<(), DriverError> {
        Ok(())
    }

    fn as_char_device(&self) -> Option<&dyn CharDevice> {
        Some(self)
    }

    fn as_char_device_mut(&mut self) -> Option<&mut dyn CharDevice> {
        Some(self)
    }
}

impl CharDevice for FramebufferConsoleDevice {
    fn read_byte(&mut self) -> Result<u8, DriverError> {
        Err(DriverError::ReadFailed)
    }

    fn write_byte(&mut self, byte: u8) -> Result<(), DriverError> {
        fb_console_write_byte(byte);
        Ok(())
    }
}
