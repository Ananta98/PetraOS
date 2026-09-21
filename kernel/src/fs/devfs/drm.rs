//! DRM Character Device & Framebuffer Node (/dev/dri/card0, /dev/fb0)
//!
//! Exposes the kernel DRM subsystem to userspace via VFS file operations:
//! - `/dev/dri/card0`: DRM card interface handling modesetting, capabilities, and ioctls.
//! - `/dev/fb0`: Primary framebuffer interface under DRM forwarding to the framebuffer driver.

use alloc::sync::Arc;
use crate::drivers::drm::{DrmCap, DrmCard, fb_read, fb_write};
use crate::fs::vfs::types::{FileOps, InodeOps, Stat, VfsError};

// DRM ioctl numbers (from Linux DRM uAPI).
const DRM_IOCTL_BASE: u64 = 0x64; // 'd'

const fn drm_io(nr: u64) -> u64 {
    (DRM_IOCTL_BASE << 8) | nr
}

// ===== DRM Card Device (/dev/dri/card0) =====

/// Inode for `/dev/dri/card0`.
pub struct DrmCardInode {
    /// Card index this inode represents.
    pub index: u32,
}

impl DrmCardInode {
    pub const fn new(index: u32) -> Self {
        Self { index }
    }
}

impl InodeOps for DrmCardInode {
    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        Ok(Arc::new(DrmCardFileOps {
            card: DrmCard::new(self.index),
        }))
    }

    fn stat(&self) -> Result<Stat, VfsError> {
        Ok(Stat {
            mode: 0o020660, // S_IFCHR | 0660
            nlink: 1,
            ..Default::default()
        })
    }
}

/// File operations for `/dev/dri/card0`.
pub struct DrmCardFileOps {
    card: DrmCard,
}

impl FileOps for DrmCardFileOps {
    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, VfsError> {
        fb_read(offset, buf)
    }

    fn write(&self, offset: usize, buf: &[u8]) -> Result<usize, VfsError> {
        fb_write(offset, buf)
    }

    fn ioctl(&self, cmd: u64, _arg: usize) -> Result<usize, VfsError> {
        // Minimal DRM ioctl surface — enough for basic modesetting clients.
        match cmd {
            // DRM_IOCTL_GET_CAP: report dumb-buffer capability.
            c if c == drm_io(0x0c) => {
                Ok(self.card.get_cap(DrmCap::DumbBuffer) as usize)
            }
            // DRM_IOCTL_VERSION: report driver version (returns 0 = success).
            c if c == drm_io(0x00) => Ok(0),
            // DRM_IOCTL_MODE_GETRESOURCES: minimal stub.
            c if c == drm_io(0xa0) => Ok(0),
            // DRM_IOCTL_MODE_CREATE_DUMB: minimal stub.
            c if c == drm_io(0xb2) => Ok(0),
            _ => Err(VfsError::NotSupported),
        }
    }
}

// ===== DRM Framebuffer Device (/dev/fb0) =====

// Linux Framebuffer ioctls
pub const FBIOGET_VSCREENINFO: u64 = 0x4600;
pub const FBIOPUT_VSCREENINFO: u64 = 0x4601;
pub const FBIOGET_FSCREENINFO: u64 = 0x4602;

/// Linux ABI struct fb_var_screeninfo layout.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct FbVarScreeninfo {
    pub xres: u32,
    pub yres: u32,
    pub xres_virtual: u32,
    pub yres_virtual: u32,
    pub xoffset: u32,
    pub yoffset: u32,
    pub bits_per_pixel: u32,
    pub grayscale: u32,
    pub red_offset: u32,
    pub red_length: u32,
    pub red_msb_right: u32,
    pub green_offset: u32,
    pub green_length: u32,
    pub green_msb_right: u32,
    pub blue_offset: u32,
    pub blue_length: u32,
    pub blue_msb_right: u32,
    pub transp_offset: u32,
    pub transp_length: u32,
    pub transp_msb_right: u32,
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

/// Linux ABI struct fb_fix_screeninfo layout.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
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

/// Inode for the `/dev/fb0` framebuffer device under DRM.
pub struct FbInode;

impl InodeOps for FbInode {
    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        Ok(Arc::new(FbFileOps))
    }

    fn stat(&self) -> Result<Stat, VfsError> {
        let size = crate::drivers::drm::get_framebuffer_info()
            .map(|fb| (fb.height * fb.pitch) as u64)
            .unwrap_or(0);

        Ok(Stat {
            mode: 0o020660, // S_IFCHR | 0660
            nlink: 1,
            size,
            ..Default::default()
        })
    }
}

/// File operations for `/dev/fb0` under DRM.
pub struct FbFileOps;

impl FileOps for FbFileOps {
    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, VfsError> {
        fb_read(offset, buf)
    }

    fn write(&self, offset: usize, buf: &[u8]) -> Result<usize, VfsError> {
        fb_write(offset, buf)
    }

    fn ioctl(&self, cmd: u64, arg: usize) -> Result<usize, VfsError> {
        let fb_info = crate::drivers::drm::get_framebuffer_info().ok_or(VfsError::NotFound)?;
        let hhdm = crate::mm::hhdm_offset();

        match cmd {
            FBIOGET_VSCREENINFO => {
                let var = FbVarScreeninfo {
                    xres: fb_info.width as u32,
                    yres: fb_info.height as u32,
                    xres_virtual: fb_info.width as u32,
                    yres_virtual: fb_info.height as u32,
                    xoffset: 0,
                    yoffset: 0,
                    bits_per_pixel: fb_info.bpp as u32,
                    grayscale: 0,
                    red_offset: fb_info.red_mask_shift as u32,
                    red_length: fb_info.red_mask_size as u32,
                    red_msb_right: 0,
                    green_offset: fb_info.green_mask_shift as u32,
                    green_length: fb_info.green_mask_size as u32,
                    green_msb_right: 0,
                    blue_offset: fb_info.blue_mask_shift as u32,
                    blue_length: fb_info.blue_mask_size as u32,
                    blue_msb_right: 0,
                    transp_offset: 0,
                    transp_length: 0,
                    transp_msb_right: 0,
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
                };
                let ptr = crate::mm::UserPtr::<FbVarScreeninfo>::from_u64(arg as u64);
                ptr.write(var).ok_or(VfsError::InvalidInput)?;
                Ok(0)
            }
            FBIOPUT_VSCREENINFO => {
                // Modesetting is currently fixed to bootloader resolution; accept without error
                Ok(0)
            }
            FBIOGET_FSCREENINFO => {
                let mut fix = FbFixScreeninfo {
                    smem_start: fb_info.address.saturating_sub(hhdm),
                    smem_len: (fb_info.height * fb_info.pitch) as u32,
                    type_: 0, // FB_TYPE_PACKED_PIXELS
                    type_aux: 0,
                    visual: 2, // FB_VISUAL_TRUECOLOR
                    xpanstep: 0,
                    ypanstep: 0,
                    ywrapstep: 0,
                    line_length: fb_info.pitch as u32,
                    mmio_start: 0,
                    mmio_len: 0,
                    accel: 0,
                    capabilities: 0,
                    reserved: [0; 2],
                    id: [0; 16],
                };
                let name = b"petra-fb";
                fix.id[..name.len()].copy_from_slice(name);

                let ptr = crate::mm::UserPtr::<FbFixScreeninfo>::from_u64(arg as u64);
                ptr.write(fix).ok_or(VfsError::InvalidInput)?;
                Ok(0)
            }
            _ => Err(VfsError::NotSupported),
        }
    }
}
