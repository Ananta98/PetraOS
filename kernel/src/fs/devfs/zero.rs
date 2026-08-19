use alloc::sync::Arc;
use crate::fs::vfs::types::{FileOps, InodeOps, VfsError};

/// Inode for the `/dev/zero` device.
pub struct ZeroInode;

impl InodeOps for ZeroInode {
    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        Ok(Arc::new(ZeroFileOps))
    }

    fn stat(&self) -> Result<crate::fs::vfs::types::Stat, VfsError> {
        Ok(crate::fs::vfs::types::Stat {
            mode: 0o020666, // S_IFCHR | 0666
            nlink: 1,
            ..Default::default()
        })
    }
}

/// File operations for `/dev/zero`.
pub struct ZeroFileOps;

impl FileOps for ZeroFileOps {
    fn read(&self, _offset: usize, buf: &mut [u8]) -> Result<usize, VfsError> {
        buf.fill(0);
        Ok(buf.len())
    }

    fn write(&self, _offset: usize, buf: &[u8]) -> Result<usize, VfsError> {
        Ok(buf.len())
    }
}
