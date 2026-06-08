use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::collections::BTreeMap;
use crate::sync::spinlock::Spinlock;
use crate::fs::errno::VfsError;
use crate::fs::vfs::inode::{Inode, InodeOps, InodeType};
use crate::fs::vfs::file::FileOps;
use super::file_ops::RamFileOps;

/// In-memory directory inode. Stores child entries in a `BTreeMap`.
pub struct RamDirInode {
    pub entries: Spinlock<BTreeMap<String, Arc<Inode>>>,
}

impl RamDirInode {
    /// Create a new empty directory inode.
    pub fn new() -> Self {
        Self {
            entries: Spinlock::new(BTreeMap::new()),
        }
    }

    /// Create a device entry within this directory (ramfs-specific, not on InodeOps).
    ///
    /// This is used by devfs to register device inodes under a ram-backed directory.
    pub fn create_device(&self, name: &str, device_ops: Arc<dyn InodeOps>, ino: u64) -> Result<Arc<Inode>, VfsError> {
        let mut entries = self.entries.lock();
        if entries.contains_key(name) {
            return Err(VfsError::AlreadyExists);
        }
        let inode = Arc::new(Inode {
            ino,
            inode_type: InodeType::CharDevice,
            ops: device_ops,
        });
        entries.insert(name.into(), inode.clone());
        Ok(inode)
    }
}

impl InodeOps for RamDirInode {
    fn lookup(&self, name: &str) -> Result<Arc<Inode>, VfsError> {
        let entries = self.entries.lock();
        entries.get(name).cloned().ok_or(VfsError::NotFound)
    }

    fn readdir(&self) -> Result<Vec<String>, VfsError> {
        let entries = self.entries.lock();
        Ok(entries.keys().cloned().collect())
    }

    fn mkdir(&self, name: &str) -> Result<Arc<Inode>, VfsError> {
        let mut entries = self.entries.lock();
        if entries.contains_key(name) {
            return Err(VfsError::AlreadyExists);
        }
        static DIR_INO: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(1000);
        let ino = DIR_INO.fetch_add(1, core::sync::atomic::Ordering::Relaxed);

        let inode = Arc::new(Inode {
            ino,
            inode_type: InodeType::Directory,
            ops: Arc::new(RamDirInode::new()),
        });
        entries.insert(name.into(), inode.clone());
        Ok(inode)
    }

    fn create(&self, name: &str) -> Result<Arc<Inode>, VfsError> {
        let mut entries = self.entries.lock();
        if entries.contains_key(name) {
            return Err(VfsError::AlreadyExists);
        }
        static FILE_INO: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(2000);
        let ino = FILE_INO.fetch_add(1, core::sync::atomic::Ordering::Relaxed);

        let inode = Arc::new(Inode {
            ino,
            inode_type: InodeType::File,
            ops: Arc::new(RamFileInode::new()),
        });
        entries.insert(name.into(), inode.clone());
        Ok(inode)
    }

    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        // Directories are not regular files; cannot be opened for I/O.
        Err(VfsError::NotFile)
    }
}

/// In-memory regular file inode. Content is shared between the inode and any
/// open file descriptions via `Arc<Spinlock<Vec<u8>>>`.
pub struct RamFileInode {
    pub content: Arc<Spinlock<Vec<u8>>>,
}

impl RamFileInode {
    /// Create a new empty file inode.
    pub fn new() -> Self {
        Self {
            content: Arc::new(Spinlock::new(Vec::new())),
        }
    }
}

impl InodeOps for RamFileInode {
    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        Ok(Arc::new(RamFileOps {
            content: self.content.clone(),
        }))
    }
}
