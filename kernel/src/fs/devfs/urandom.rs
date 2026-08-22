use crate::arch::cpu::rdtsc;
use crate::fs::vfs::types;
use crate::fs::vfs::types::{FileOps, InodeOps, VfsError};
use alloc::sync::Arc;

/// Inode for the `/dev/urandom` device.
pub struct UrandomInode;

impl InodeOps for UrandomInode {
    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        Ok(Arc::new(UrandomFileOps))
    }

    fn stat(&self) -> Result<types::Stat, VfsError> {
        Ok(types::Stat {
            mode: 0o020666, // S_IFCHR | 0666
            nlink: 1,
            ..Default::default()
        })
    }
}

/// File operations for `/dev/urandom`.
pub struct UrandomFileOps;

impl FileOps for UrandomFileOps {
    fn read(&self, _offset: usize, buf: &mut [u8]) -> Result<usize, VfsError> {
        rdtsc::fill_random_bytes(buf);
        Ok(buf.len())
    }

    fn write(&self, _offset: usize, buf: &[u8]) -> Result<usize, VfsError> {
        Ok(buf.len())
    }
}
