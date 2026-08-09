use super::inode::Ext2Volume;
use crate::fs::vfs::types::{FileOps, SeekWhence, Stat, VfsError};
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

    /// Write file content starting from absolute offset.
    fn write(&self, offset: usize, buf: &[u8]) -> Result<usize, VfsError> {
        let mut inode = self.volume.read_inode(self.ino)?;
        self.volume.write_inode_data(&mut inode, self.ino, offset, buf)
    }

    /// Truncate file to target size.
    fn truncate(&self, size: usize) -> Result<(), VfsError> {
        let mut inode = self.volume.read_inode(self.ino)?;
        inode.size = size as u32;
        self.volume.write_inode(self.ino, &inode)
    }

    /// Fetch stat metadata for Ext2 file.
    fn stat(&self) -> Result<Stat, VfsError> {
        let inode = self.volume.read_inode(self.ino)?;
        Ok(Stat {
            ino: self.ino as u64,
            mode: inode.mode as u32,
            nlink: inode.links_count as u32,
            size: inode.size as u64,
            atime: inode.atime as u64,
            mtime: inode.mtime as u64,
            ctime: inode.ctime as u64,
            blksize: self.volume.sb.block_size as u64,
            blocks: inode.blocks as u64,
            ..Default::default()
        })
    }

    /// Synchronize changes to disk.
    fn sync(&self) -> Result<(), VfsError> {
        Ok(())
    }
}
