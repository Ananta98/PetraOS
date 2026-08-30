//! Framebuffer Driver & Abstraction (/dev/fb0)
//!
//! Provides a unified device driver interface for linear framebuffers supplied
//! by the Limine bootloader protocol.

use crate::device::{Device, DeviceType, Driver, DriverError};
use crate::fs::vfs::types::VfsError;
use crate::limine::FRAMEBUFFER_REQUEST;
use crate::sync::Mutex;
use alloc::boxed::Box;
use alloc::sync::Arc;

static FB_DEVICE: Mutex<Option<FramebufferDevice>> = Mutex::new(None);

/// Raw metadata describing the display framebuffer.
#[derive(Debug, Clone, Copy, Default)]
pub struct FramebufferInfo {
    pub address: u64,
    pub width: u64,
    pub height: u64,
    pub pitch: u64,
    pub bpp: u16,
    pub red_mask_size: u8,
    pub red_mask_shift: u8,
    pub green_mask_size: u8,
    pub green_mask_shift: u8,
    pub blue_mask_size: u8,
    pub blue_mask_shift: u8,
}

/// OOP Framebuffer Device Abstraction.
pub struct FramebufferDevice {
    info: FramebufferInfo,
}

impl FramebufferDevice {
    pub const fn new(info: FramebufferInfo) -> Self {
        Self { info }
    }

    pub fn info(&self) -> &FramebufferInfo {
        &self.info
    }

    pub fn size_bytes(&self) -> usize {
        (self.info.height * self.info.pitch) as usize
    }

    pub fn read_bytes(&self, offset: usize, buf: &mut [u8]) -> Result<usize, VfsError> {
        let total_size = self.size_bytes();
        if offset >= total_size {
            return Ok(0);
        }
        let copy_len = core::cmp::min(buf.len(), total_size - offset);
        let src_ptr = (self.info.address as usize + offset) as *const u8;
        // SAFETY: Pointer is within linear framebuffer bounds queried from bootloader.
        unsafe {
            core::ptr::copy_nonoverlapping(src_ptr, buf.as_mut_ptr(), copy_len);
        }
        Ok(copy_len)
    }

    pub fn write_bytes(&self, offset: usize, buf: &[u8]) -> Result<usize, VfsError> {
        let total_size = self.size_bytes();
        if offset >= total_size {
            return Ok(0);
        }
        let copy_len = core::cmp::min(buf.len(), total_size - offset);
        let dst_ptr = (self.info.address as usize + offset) as *mut u8;
        // SAFETY: Pointer is within linear framebuffer bounds queried from bootloader.
        unsafe {
            core::ptr::copy_nonoverlapping(buf.as_ptr(), dst_ptr, copy_len);
        }
        Ok(copy_len)
    }
}

impl Device for FramebufferDevice {
    fn dev_type(&self) -> DeviceType {
        DeviceType::Gpu
    }

    fn name(&self) -> &'static str {
        "Limine Linear Framebuffer"
    }

    fn dev_name(&self) -> Option<&'static str> {
        Some("fb0")
    }

    fn init(&mut self) -> Result<(), DriverError> {
        Ok(())
    }
}

/// Query bootloader framebuffer information if available.
pub fn get_framebuffer_info() -> Option<FramebufferInfo> {
    if let Some(guard) = FB_DEVICE.lock().as_ref() {
        return Some(*guard.info());
    }

    if let Some(fb_response) = FRAMEBUFFER_REQUEST.get_response() {
        if let Some(fb) = fb_response.framebuffers().next() {
            let info = FramebufferInfo {
                address: fb.addr() as u64,
                width: fb.width(),
                height: fb.height(),
                pitch: fb.pitch(),
                bpp: fb.bpp(),
                red_mask_size: fb.red_mask_size(),
                red_mask_shift: fb.red_mask_shift(),
                green_mask_size: fb.green_mask_size(),
                green_mask_shift: fb.green_mask_shift(),
                blue_mask_size: fb.blue_mask_size(),
                blue_mask_shift: fb.blue_mask_shift(),
            };
            return Some(info);
        }
    }
    None
}

/// Initialize the framebuffer device driver.
pub fn init() {
    if FB_DEVICE.lock().is_some() {
        return;
    }
    if let Some(info) = get_framebuffer_info() {
        let dev = FramebufferDevice::new(info);
        *FB_DEVICE.lock() = Some(dev);
    }
}

/// VFS bridge function: read raw framebuffer memory.
pub fn fb_read(offset: usize, buf: &mut [u8]) -> Result<usize, VfsError> {
    let guard = FB_DEVICE.lock();
    if let Some(ref dev) = *guard {
        dev.read_bytes(offset, buf)
    } else {
        Err(VfsError::NotFound)
    }
}

/// VFS bridge function: write raw framebuffer memory.
pub fn fb_write(offset: usize, buf: &[u8]) -> Result<usize, VfsError> {
    let guard = FB_DEVICE.lock();
    if let Some(ref dev) = *guard {
        dev.write_bytes(offset, buf)
    } else {
        Err(VfsError::NotFound)
    }
}

/// Framebuffer driver struct for device manager registration.
#[derive(Default)]
pub struct FramebufferDriver;

impl Driver for FramebufferDriver {
    fn name(&self) -> &'static str {
        "framebuffer_driver"
    }

    fn bus_name(&self) -> &'static str {
        "platform"
    }

    fn description(&self) -> &'static str {
        "Limine Linear Framebuffer Driver"
    }

    fn probe(&self) -> Result<(), DriverError> {
        init();
        if let Some(info) = get_framebuffer_info() {
            let dev = FramebufferDevice::new(info);
            let dev_ref: Arc<Mutex<Box<dyn Device>>> = Arc::new(Mutex::new(Box::new(dev)));
            crate::device::DEVICE_MANAGER.write().register(dev_ref);
            log::info!(
                "[FB] Framebuffer probed: {}x{} @ {} bpp",
                info.width,
                info.height,
                info.bpp
            );
        }
        Ok(())
    }
}

crate::MODULE_LICENSE!("BSD-2-Clause");
crate::MODULE_AUTHOR!("Ananta98");
crate::MODULE_DESCRIPTION!("Limine Linear Framebuffer Driver");
crate::MODULE_VERSION!("1.0.0");
crate::module_driver!(
    FB_INITCALL,
    framebuffer_driver_init,
    "framebuffer_driver",
    FramebufferDriver
);
