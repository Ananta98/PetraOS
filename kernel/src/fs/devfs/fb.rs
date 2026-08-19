use alloc::sync::Arc;
use crate::drivers::gpu::framebuffer::{fb_read, fb_write};
use crate::fs::vfs::types::{FileOps, InodeOps, VfsError};

/// Inode for the `/dev/fb0` framebuffer device.
pub struct FbInode;

impl InodeOps for FbInode {
    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        Ok(Arc::new(FbFileOps))
    }

    fn stat(&self) -> Result<crate::fs::vfs::types::Stat, VfsError> {
        Ok(crate::fs::vfs::types::Stat {
            mode: 0o020660, // S_IFCHR | 0660
            nlink: 1,
            ..Default::default()
        })
    }
}

/// File operations for `/dev/fb0`.
pub struct FbFileOps;

impl FileOps for FbFileOps {
    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, VfsError> {
        fb_read(offset, buf)
    }

    fn write(&self, offset: usize, buf: &[u8]) -> Result<usize, VfsError> {
        fb_write(offset, buf)
    }
}

