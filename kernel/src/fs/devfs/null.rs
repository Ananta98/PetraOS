use alloc::sync::Arc;
use crate::fs::vfs::types::{FileOps, InodeOps, VfsError};

/// Inode for the `/dev/null` device.
pub struct NullInode;

impl InodeOps for NullInode {
    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        Ok(Arc::new(NullFileOps))
    }
}

/// File operations for `/dev/null`.
pub struct NullFileOps;

impl FileOps for NullFileOps {
    fn read(&self, _offset: usize, _buf: &mut [u8]) -> Result<usize, VfsError> {
        Ok(0)
    }

    fn write(&self, _offset: usize, buf: &[u8]) -> Result<usize, VfsError> {
        Ok(buf.len())
    }
}
