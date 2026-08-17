use alloc::sync::Arc;
use crate::drivers::gpu::framebuffer::FRAMEBUFFER;
use crate::fs::vfs::types::{FileOps, InodeOps, VfsError};

/// Inode for the `/dev/fb0` framebuffer device.
pub struct FbInode;

impl InodeOps for FbInode {
    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        Ok(Arc::new(FbFileOps))
    }
}

/// File operations for `/dev/fb0`.
pub struct FbFileOps;

impl FileOps for FbFileOps {
    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, VfsError> {
        let fb_guard = FRAMEBUFFER.lock();
        let fb = fb_guard.as_ref().ok_or(VfsError::NotFound)?;
        let total_len = fb.len();
        if offset >= total_len {
            return Ok(0);
        }
        let available = total_len - offset;
        let count = core::cmp::min(buf.len(), available);
        // SAFETY: Pointer is within mapped framebuffer memory.
        unsafe {
            core::ptr::copy_nonoverlapping(fb.info().addr.add(offset), buf.as_mut_ptr(), count);
        }
        Ok(count)
    }

    fn write(&self, offset: usize, buf: &[u8]) -> Result<usize, VfsError> {
        let fb_guard = FRAMEBUFFER.lock();
        let fb = fb_guard.as_ref().ok_or(VfsError::NotFound)?;
        let total_len = fb.len();
        if offset >= total_len {
            return Ok(0);
        }
        let available = total_len - offset;
        let count = core::cmp::min(buf.len(), available);
        // SAFETY: Pointer is within mapped framebuffer memory.
        unsafe {
            core::ptr::copy_nonoverlapping(buf.as_ptr(), fb.info().addr.add(offset), count);
        }
        Ok(count)
    }
}
