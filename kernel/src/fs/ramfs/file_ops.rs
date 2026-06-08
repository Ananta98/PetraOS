use alloc::sync::Arc;
use alloc::vec::Vec;
use crate::sync::spinlock::Spinlock;
use crate::fs::errno::VfsError;
use crate::fs::vfs::file::FileOps;

/// File operations for ramfs regular files.
///
/// Reads from and writes to an in-memory `Vec<u8>` shared with the
/// [`RamFileInode`](super::inode::RamFileInode) that produced this instance.
pub struct RamFileOps {
    pub content: Arc<Spinlock<Vec<u8>>>,
}

impl FileOps for RamFileOps {
    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, VfsError> {
        let content = self.content.lock();
        if offset >= content.len() {
            return Ok(0);
        }
        let len = core::cmp::min(buf.len(), content.len() - offset);
        buf[..len].copy_from_slice(&content[offset..offset + len]);
        Ok(len)
    }

    fn write(&self, offset: usize, buf: &[u8]) -> Result<usize, VfsError> {
        let mut content = self.content.lock();
        let end = offset + buf.len();
        if end > content.len() {
            content.resize(end, 0);
        }
        content[offset..end].copy_from_slice(buf);
        Ok(buf.len())
    }
}
