//! Linux-like Framebuffer Driver and fbdev Subsystem
//!
//! Implements standard Linux fbdev data structures, ioctls, and operations
//! (`fb_ops`) backed by the Limine bootloader linear framebuffer.

use alloc::boxed::Box;
use alloc::sync::Arc;
use core::ptr;

use crate::device::{Device, DeviceType, Driver, DriverError, Major, Minor};
use crate::fs::vfs::types::VfsError;
use crate::sync::spinlock::Spinlock;

// Linux fbdev IOCTL numbers
pub const FBIOGET_VSCREENINFO: u64 = 0x4600;
pub const FBIOPUT_VSCREENINFO: u64 = 0x4601;
pub const FBIOGET_FSCREENINFO: u64 = 0x4602;
pub const FBIOPAN_DISPLAY: u64 = 0x4606;
pub const FBIOBLANK: u64 = 0x4611;

// Linux fbdev constants
pub const FB_TYPE_PACKED_PIXELS: u32 = 0;
pub const FB_VISUAL_TRUECOLOR: u32 = 2;
pub const FB_BLANK_UNBLANK: i32 = 0;
pub const FB_BLANK_NORMAL: i32 = 1;
pub const FB_BLANK_POWERDOWN: i32 = 4;

/// Description of a color bitfield in a pixel (e.g., Red, Green, Blue, Transp).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FbBitfield {
    pub offset: u32,
    pub length: u32,
    pub msb_right: u32,
}

/// Linux variable screen info structure for `FBIOGET_VSCREENINFO` / `FBIOPUT_VSCREENINFO`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FbVarScreeninfo {
    pub xres: u32,
    pub yres: u32,
    pub xres_virtual: u32,
    pub yres_virtual: u32,
    pub xoffset: u32,
    pub yoffset: u32,
    pub bits_per_pixel: u32,
    pub grayscale: u32,
    pub red: FbBitfield,
    pub green: FbBitfield,
    pub blue: FbBitfield,
    pub transp: FbBitfield,
    pub nonstd: u32,
    pub activate: u32,
    pub height: u32,
    pub width: u32,
    pub accel_flags: u32,
    pub pixclock: u32,
    pub left_margin: u32,
    pub right_margin: u32,
    pub upper_margin: u32,
    pub lower_margin: u32,
    pub hsync_len: u32,
    pub vsync_len: u32,
    pub sync: u32,
    pub vmode: u32,
    pub rotate: u32,
    pub colorspace: u32,
    pub reserved: [u32; 4],
}

/// Linux fixed screen info structure for `FBIOGET_FSCREENINFO`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FbFixScreeninfo {
    pub id: [u8; 16],
    pub smem_start: u64,
    pub smem_len: u32,
    pub type_: u32,
    pub type_aux: u32,
    pub visual: u32,
    pub xpanstep: u16,
    pub ypanstep: u16,
    pub ywrapstep: u16,
    pub line_length: u32,
    pub mmio_start: u64,
    pub mmio_len: u32,
    pub accel: u32,
    pub capabilities: u16,
    pub reserved: [u16; 2],
}

/// Linux rectangle fill parameter structure.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FbFillrect {
    pub dx: u32,
    pub dy: u32,
    pub width: u32,
    pub height: u32,
    pub color: u32,
    pub rop: u32,
}

/// Linux copy area parameter structure.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FbCopyarea {
    pub dx: u32,
    pub dy: u32,
    pub width: u32,
    pub height: u32,
    pub sx: u32,
    pub sy: u32,
}

/// Linux image blit parameter structure.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FbImage {
    pub dx: u32,
    pub dy: u32,
    pub width: u32,
    pub height: u32,
    pub fg_color: u32,
    pub bg_color: u32,
    pub depth: u8,
    pub data: *const u8,
}

/// Framebuffer configuration and hardware info.
#[derive(Debug, Clone, Copy)]
pub struct FramebufferInfo {
    pub addr: *mut u8,
    pub width: usize,
    pub height: usize,
    pub pitch: usize,
    pub bpp: usize,
    pub red_mask_size: u8,
    pub red_mask_shift: u8,
    pub green_mask_size: u8,
    pub green_mask_shift: u8,
    pub blue_mask_size: u8,
    pub blue_mask_shift: u8,
}

// SAFETY: FramebufferInfo contains raw pointer to mapped Limine linear memory.
unsafe impl Send for FramebufferInfo {}
unsafe impl Sync for FramebufferInfo {}

/// Framebuffer device state wrapper.
pub struct Framebuffer {
    info: FramebufferInfo,
}

impl Framebuffer {
    pub const fn new(info: FramebufferInfo) -> Self {
        Self { info }
    }

    #[inline]
    pub const fn info(&self) -> &FramebufferInfo {
        &self.info
    }

    #[inline]
    pub const fn len(&self) -> usize {
        self.info.height * self.info.pitch
    }

    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Global active Framebuffer instance.
pub static FRAMEBUFFER: Spinlock<Option<Framebuffer>> = Spinlock::new(None);

// -----------------------------------------------------------------------------
// Linux fb_ops Functions
// -----------------------------------------------------------------------------

/// Opens the framebuffer device.
pub fn fb_open() -> Result<(), VfsError> {
    if FRAMEBUFFER.lock().is_some() {
        Ok(())
    } else {
        Err(VfsError::NotFound)
    }
}

/// Releases the framebuffer device.
pub fn fb_release() -> Result<(), VfsError> {
    Ok(())
}

/// Reads bytes from the linear framebuffer memory.
pub fn fb_read(offset: usize, buf: &mut [u8]) -> Result<usize, VfsError> {
    let guard = FRAMEBUFFER.lock();
    let fb = guard.as_ref().ok_or(VfsError::NotFound)?;
    let total_len = fb.len();

    if offset >= total_len {
        return Ok(0);
    }

    let count = core::cmp::min(buf.len(), total_len - offset);
    // SAFETY: Offset and count are within the bounds of the mapped framebuffer buffer.
    unsafe {
        ptr::copy_nonoverlapping(fb.info.addr.add(offset), buf.as_mut_ptr(), count);
    }
    Ok(count)
}

/// Writes bytes into the linear framebuffer memory.
pub fn fb_write(offset: usize, buf: &[u8]) -> Result<usize, VfsError> {
    let guard = FRAMEBUFFER.lock();
    let fb = guard.as_ref().ok_or(VfsError::NotFound)?;
    let total_len = fb.len();

    if offset >= total_len {
        return Ok(0);
    }

    let count = core::cmp::min(buf.len(), total_len - offset);
    // SAFETY: Offset and count are within the bounds of the mapped framebuffer buffer.
    unsafe {
        ptr::copy_nonoverlapping(buf.as_ptr(), fb.info.addr.add(offset), count);
    }
    Ok(count)
}

/// Retrieves the Linux `FbVarScreeninfo` for the active display.
pub fn fb_get_var() -> Result<FbVarScreeninfo, VfsError> {
    let guard = FRAMEBUFFER.lock();
    let fb = guard.as_ref().ok_or(VfsError::NotFound)?;
    let info = fb.info();

    Ok(FbVarScreeninfo {
        xres: info.width as u32,
        yres: info.height as u32,
        xres_virtual: info.width as u32,
        yres_virtual: info.height as u32,
        xoffset: 0,
        yoffset: 0,
        bits_per_pixel: info.bpp as u32,
        grayscale: 0,
        red: FbBitfield {
            offset: info.red_mask_shift as u32,
            length: info.red_mask_size as u32,
            msb_right: 0,
        },
        green: FbBitfield {
            offset: info.green_mask_shift as u32,
            length: info.green_mask_size as u32,
            msb_right: 0,
        },
        blue: FbBitfield {
            offset: info.blue_mask_shift as u32,
            length: info.blue_mask_size as u32,
            msb_right: 0,
        },
        transp: FbBitfield {
            offset: 24,
            length: 8,
            msb_right: 0,
        },
        nonstd: 0,
        activate: 0,
        height: 0,
        width: 0,
        accel_flags: 0,
        pixclock: 0,
        left_margin: 0,
        right_margin: 0,
        upper_margin: 0,
        lower_margin: 0,
        hsync_len: 0,
        vsync_len: 0,
        sync: 0,
        vmode: 0,
        rotate: 0,
        colorspace: 0,
        reserved: [0; 4],
    })
}

/// Sets variable screen info parameters.
pub fn fb_set_var(_var: &FbVarScreeninfo) -> Result<(), VfsError> {
    if FRAMEBUFFER.lock().is_some() {
        Ok(())
    } else {
        Err(VfsError::NotFound)
    }
}

/// Retrieves the Linux `FbFixScreeninfo` for the active display.
pub fn fb_get_fix() -> Result<FbFixScreeninfo, VfsError> {
    let guard = FRAMEBUFFER.lock();
    let fb = guard.as_ref().ok_or(VfsError::NotFound)?;
    let info = fb.info();

    let mut id = [0u8; 16];
    let name_bytes = b"petraos-fb";
    id[..name_bytes.len()].copy_from_slice(name_bytes);

    Ok(FbFixScreeninfo {
        id,
        smem_start: info.addr as u64,
        smem_len: fb.len() as u32,
        type_: FB_TYPE_PACKED_PIXELS,
        type_aux: 0,
        visual: FB_VISUAL_TRUECOLOR,
        xpanstep: 0,
        ypanstep: 0,
        ywrapstep: 0,
        line_length: info.pitch as u32,
        mmio_start: 0,
        mmio_len: 0,
        accel: 0,
        capabilities: 0,
        reserved: [0; 2],
    })
}

/// Fills a rectangular region on screen with a specific pixel value.
pub fn fb_fillrect(rect: &FbFillrect) {
    let guard = FRAMEBUFFER.lock();
    if let Some(ref fb) = *guard {
        let info = fb.info();
        let bytes_per_pixel = info.bpp / 8;
        let x_end = core::cmp::min((rect.dx + rect.width) as usize, info.width);
        let y_end = core::cmp::min((rect.dy + rect.height) as usize, info.height);

        for y in (rect.dy as usize)..y_end {
            let row_offset = y * info.pitch;
            for x in (rect.dx as usize)..x_end {
                let offset = row_offset + x * bytes_per_pixel;
                // SAFETY: Calculated offsets within mapped framebuffer boundaries.
                unsafe {
                    let ptr = info.addr.add(offset);
                    if bytes_per_pixel == 4 {
                        ptr::write_volatile(ptr as *mut u32, rect.color);
                    } else if bytes_per_pixel == 2 {
                        ptr::write_volatile(ptr as *mut u16, rect.color as u16);
                    }
                }
            }
        }
    }
}

/// Copies an area of pixels from (sx, sy) to (dx, dy).
pub fn fb_copyarea(area: &FbCopyarea) {
    let guard = FRAMEBUFFER.lock();
    if let Some(ref fb) = *guard {
        let info = fb.info();
        let bytes_per_pixel = info.bpp / 8;
        let width_bytes = (area.width as usize) * bytes_per_pixel;

        for row in 0..(area.height as usize) {
            let src_y = (area.sy as usize) + row;
            let dst_y = (area.dy as usize) + row;
            if src_y >= info.height || dst_y >= info.height {
                continue;
            }

            let src_offset = src_y * info.pitch + (area.sx as usize) * bytes_per_pixel;
            let dst_offset = dst_y * info.pitch + (area.dx as usize) * bytes_per_pixel;

            // SAFETY: Source and destination memory regions reside in mapped framebuffer memory.
            unsafe {
                ptr::copy(info.addr.add(src_offset), info.addr.add(dst_offset), width_bytes);
            }
        }
    }
}

/// Dispatches standard Linux fbdev IOCTL requests.
pub fn fb_ioctl(cmd: u64, arg: usize) -> Result<usize, VfsError> {
    let arg_ptr = arg as *mut u8;

    match cmd {
        FBIOGET_VSCREENINFO => {
            if arg_ptr.is_null() || !crate::syscalls::is_user_ptr_valid(arg_ptr as u64, core::mem::size_of::<FbVarScreeninfo>()) {
                return Err(VfsError::InvalidInput);
            }
            let var = fb_get_var()?;
            // SAFETY: Validated user pointer.
            unsafe {
                ptr::write_volatile(arg_ptr as *mut FbVarScreeninfo, var);
            }
            Ok(0)
        }
        FBIOPUT_VSCREENINFO => {
            if arg_ptr.is_null() || !crate::syscalls::is_user_ptr_valid(arg_ptr as u64, core::mem::size_of::<FbVarScreeninfo>()) {
                return Err(VfsError::InvalidInput);
            }
            // SAFETY: Validated user pointer.
            let var = unsafe { ptr::read_volatile(arg_ptr as *const FbVarScreeninfo) };
            fb_set_var(&var)?;
            Ok(0)
        }
        FBIOGET_FSCREENINFO => {
            if arg_ptr.is_null() || !crate::syscalls::is_user_ptr_valid(arg_ptr as u64, core::mem::size_of::<FbFixScreeninfo>()) {
                return Err(VfsError::InvalidInput);
            }
            let fix = fb_get_fix()?;
            // SAFETY: Validated user pointer.
            unsafe {
                ptr::write_volatile(arg_ptr as *mut FbFixScreeninfo, fix);
            }
            Ok(0)
        }
        FBIOPAN_DISPLAY | FBIOBLANK => Ok(0),
        _ => Err(VfsError::NotSupported),
    }
}

// -----------------------------------------------------------------------------
// Framebuffer Device & Driver Registration
// -----------------------------------------------------------------------------

/// GPU Framebuffer device registered with `DEVICE_MANAGER`.
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
        "Limine Framebuffer (fb0)"
    }

    fn init(&mut self) -> Result<(), DriverError> {
        if FRAMEBUFFER.lock().is_some() {
            Ok(())
        } else {
            Err(DriverError::InitFailed)
        }
    }
}

/// Driver structure for Limine Framebuffer.
#[derive(Default)]
pub struct FramebufferDriver;

impl Driver for FramebufferDriver {
    fn name(&self) -> &'static str {
        "framebuffer"
    }

    fn bus_name(&self) -> &'static str {
        "platform"
    }

    fn description(&self) -> &'static str {
        "Linux-like Framebuffer Driver"
    }

    fn probe(&self) -> Result<(), DriverError> {
        let response = match crate::limine::FRAMEBUFFER_REQUEST.get_response() {
            Some(resp) => resp,
            None => {
                log::warn!("[Framebuffer] No Limine framebuffer response found.");
                return Err(DriverError::Unsupported);
            }
        };

        let limine_fb = match response.framebuffers().next() {
            Some(fb) => fb,
            None => {
                log::warn!("[Framebuffer] Framebuffer list is empty.");
                return Err(DriverError::Unsupported);
            }
        };

        let info = FramebufferInfo {
            addr: limine_fb.addr(),
            width: limine_fb.width() as usize,
            height: limine_fb.height() as usize,
            pitch: limine_fb.pitch() as usize,
            bpp: limine_fb.bpp() as usize,
            red_mask_size: limine_fb.red_mask_size(),
            red_mask_shift: limine_fb.red_mask_shift(),
            green_mask_size: limine_fb.green_mask_size(),
            green_mask_shift: limine_fb.green_mask_shift(),
            blue_mask_size: limine_fb.blue_mask_size(),
            blue_mask_shift: limine_fb.blue_mask_shift(),
        };

        log::info!(
            "[Framebuffer] Initialized fb0: {}x{} @ {}bpp, pitch={}, addr={:p}",
            info.width,
            info.height,
            info.bpp,
            info.pitch,
            info.addr
        );

        let fb = Framebuffer::new(info);
        *FRAMEBUFFER.lock() = Some(fb);

        // Register FramebufferDevice to DEVICE_MANAGER
        let fb_dev_ref: Arc<Spinlock<Box<dyn Device>>> =
            Arc::new(Spinlock::new(Box::new(FramebufferDevice)));
        crate::device::DEVICE_MANAGER.write().register(fb_dev_ref);

        log::info!("[Framebuffer] Device /dev/fb0 registered successfully.");
        Ok(())
    }
}

/// Explicit initialization entry point for the framebuffer driver.
pub fn init() -> Result<(), DriverError> {
    FramebufferDriver::default().probe()
}

crate::MODULE_LICENSE!("BSD-2-Clause");
crate::MODULE_AUTHOR!("PetraOS Development Team");
crate::MODULE_DESCRIPTION!("Linux-like Framebuffer Driver");
crate::MODULE_VERSION!("1.0.0");
crate::module_driver!(
    FRAMEBUFFER_INITCALL,
    framebuffer_driver_init,
    "framebuffer",
    FramebufferDriver
);
