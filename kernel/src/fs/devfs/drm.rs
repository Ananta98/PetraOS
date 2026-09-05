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

/// Inode for the `/dev/fb0` framebuffer device under DRM.
pub struct FbInode;

impl InodeOps for FbInode {
    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        Ok(Arc::new(FbFileOps))
    }

    fn stat(&self) -> Result<Stat, VfsError> {
        Ok(Stat {
            mode: 0o020660, // S_IFCHR | 0660
            nlink: 1,
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
}
