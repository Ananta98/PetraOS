use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::collections::BTreeMap;
use crate::sync::spinlock::Spinlock;
use crate::fs::errno::VfsError;
use crate::fs::vfs::inode::{Inode, InodeOps};
use crate::fs::vfs::file::FileOps;

/// Directory inode for procfs. Contains a fixed set of entries populated at mount time.
pub struct ProcDirInode {
    pub entries: Spinlock<BTreeMap<String, Arc<Inode>>>,
}

impl ProcDirInode {
    /// Create a new empty proc directory inode.
    pub fn new() -> Self {
        Self {
            entries: Spinlock::new(BTreeMap::new()),
        }
    }

    /// Add a child inode to this directory.
    pub fn add_entry(&self, name: &str, inode: Arc<Inode>) {
        self.entries.lock().insert(name.into(), inode);
    }
}

impl InodeOps for ProcDirInode {
    fn lookup(&self, name: &str) -> Result<Arc<Inode>, VfsError> {
        let entries = self.entries.lock();
        entries.get(name).cloned().ok_or(VfsError::NotFound)
    }

    fn readdir(&self) -> Result<Vec<String>, VfsError> {
        let entries = self.entries.lock();
        Ok(entries.keys().cloned().collect())
    }

    fn create(&self, _name: &str) -> Result<Arc<Inode>, VfsError> {
        Err(VfsError::ReadOnlyFs)
    }

    fn mkdir(&self, _name: &str) -> Result<Arc<Inode>, VfsError> {
        Err(VfsError::ReadOnlyFs)
    }

    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        Err(VfsError::NotFile)
    }
}

/// Read-only file inode for procfs. Content is a static byte slice.
pub struct ProcFileInode {
    pub content: &'static [u8],
}

impl InodeOps for ProcFileInode {
    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        Ok(Arc::new(ProcFileOps {
            content: self.content,
        }))
    }
}

/// File operations for a read-only procfs file backed by a static byte slice.
pub struct ProcFileOps {
    content: &'static [u8],
}

impl FileOps for ProcFileOps {
    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, VfsError> {
        if offset >= self.content.len() {
            return Ok(0);
        }
        let len = core::cmp::min(buf.len(), self.content.len() - offset);
        buf[..len].copy_from_slice(&self.content[offset..offset + len]);
        Ok(len)
    }

    fn write(&self, _offset: usize, _buf: &[u8]) -> Result<usize, VfsError> {
        Err(VfsError::ReadOnlyFs)
    }
}
