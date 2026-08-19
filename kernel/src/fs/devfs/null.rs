use alloc::sync::Arc;
use crate::fs::vfs::types::{FileOps, InodeOps, VfsError};

/// Inode for the `/dev/null` device.
pub struct NullInode;

impl InodeOps for NullInode {
    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        Ok(Arc::new(NullFileOps))
    }

    fn stat(&self) -> Result<crate::fs::vfs::types::Stat, VfsError> {
        Ok(crate::fs::vfs::types::Stat {
            mode: 0o020666, // S_IFCHR | 0666
            nlink: 1,
            ..Default::default()
        })
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
