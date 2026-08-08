use super::inode::Ext2Volume;
use crate::fs::vfs::types::{FileOps, VfsError};
use alloc::sync::Arc;

/// File operations (I/O) dispatch table for Ext2.
pub struct Ext2FileOps {
    pub volume: Arc<Ext2Volume>,
    pub ino: u32,
}

impl FileOps for Ext2FileOps {
    /// Read file content starting from absolute offset.
    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, VfsError> {
        let inode = self.volume.read_inode(self.ino)?;
        self.volume.read_inode_data(&inode, offset, buf)
    }

    /// Write is not supported on read-only Ext2.
    fn write(&self, _offset: usize, _buf: &[u8]) -> Result<usize, VfsError> {
        Err(VfsError::ReadOnlyFs)
    }
}
