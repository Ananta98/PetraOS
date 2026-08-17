use alloc::sync::Arc;
use crate::fs::vfs::types::{FileOps, InodeOps, VfsError};

/// Inode for the `/dev/urandom` device.
pub struct UrandomInode;

impl InodeOps for UrandomInode {
    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        Ok(Arc::new(UrandomFileOps))
    }
}

/// File operations for `/dev/urandom`.
pub struct UrandomFileOps;

impl FileOps for UrandomFileOps {
    fn read(&self, _offset: usize, buf: &mut [u8]) -> Result<usize, VfsError> {
        crate::arch::cpu::rdtsc::fill_random_bytes(buf);
        Ok(buf.len())
    }

    fn write(&self, _offset: usize, buf: &[u8]) -> Result<usize, VfsError> {
        Ok(buf.len())
    }
}
