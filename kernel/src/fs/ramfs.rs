use crate::fs::vfs::types::{FileOps, FileSystem, SuperBlock, VfsError};
use crate::fs::{Inode, InodeOps, InodeType};
use crate::sync::rwlock::RwLock;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

/// In-memory regular file ops.
pub struct RamFileOps {
    pub content: Arc<RwLock<Vec<u8>>>,
}

impl FileOps for RamFileOps {
    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, VfsError> {
        let content = self.content.read();
        if offset >= content.len() {
            return Ok(0);
        }
        let len = core::cmp::min(buf.len(), content.len() - offset);
        buf[..len].copy_from_slice(&content[offset..offset + len]);
        Ok(len)
    }

    fn write(&self, offset: usize, buf: &[u8]) -> Result<usize, VfsError> {
        let mut content = self.content.write();
        let end = offset + buf.len();
        if end > content.len() {
            content.resize(end, 0);
        }
        content[offset..end].copy_from_slice(buf);
        Ok(buf.len())
    }

    fn truncate(&self, size: usize) -> Result<(), VfsError> {
        let mut content = self.content.write();
        content.resize(size, 0);
        Ok(())
    }

    fn stat(&self) -> Result<crate::fs::vfs::types::Stat, VfsError> {
        let content = self.content.read();
        Ok(crate::fs::vfs::types::Stat {
            size: content.len() as u64,
            mode: 0o100644,
            nlink: 1,
            ..Default::default()
        })
    }
}

/// In-memory regular file inode.
pub struct RamFileInode {
    pub content: Arc<RwLock<Vec<u8>>>,
}

impl RamFileInode {
    pub fn new() -> Self {
        Self {
            content: Arc::new(RwLock::new(Vec::new())),
        }
    }
}

impl InodeOps for RamFileInode {
    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        Ok(Arc::new(RamFileOps {
            content: self.content.clone(),
        }))
    }

    fn stat(&self) -> Result<crate::fs::vfs::types::Stat, VfsError> {
        let content = self.content.read();
        Ok(crate::fs::vfs::types::Stat {
            size: content.len() as u64,
            mode: 0o100644,
            nlink: 1,
            ..Default::default()
        })
    }

    fn truncate(&self, size: usize) -> Result<(), VfsError> {
        let mut content = self.content.write();
        content.resize(size, 0);
        Ok(())
    }
}

/// In-memory directory inode. Stores child entries in a `BTreeMap`.
pub struct RamDirInode {
    pub entries: RwLock<BTreeMap<String, Arc<Inode>>>,
}

impl RamDirInode {
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(BTreeMap::new()),
        }
    }

    pub fn create_device(
        &self,
        name: &str,
        device_ops: Arc<dyn InodeOps>,
        ino: u64,
    ) -> Result<Arc<Inode>, VfsError> {
        let mut entries = self.entries.write();
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
        let entries = self.entries.read();
        entries.get(name).cloned().ok_or(VfsError::NotFound)
    }

    fn readdir(&self) -> Result<Vec<String>, VfsError> {
        let entries = self.entries.read();
        Ok(entries.keys().cloned().collect())
    }

    fn mkdir(&self, name: &str) -> Result<Arc<Inode>, VfsError> {
        let mut entries = self.entries.write();
        if entries.contains_key(name) {
            return Err(VfsError::AlreadyExists);
        }
        static DIR_INO: AtomicU64 = AtomicU64::new(1000);
        let ino = DIR_INO.fetch_add(1, Ordering::Relaxed);

        let inode = Arc::new(Inode {
            ino,
            inode_type: InodeType::Directory,
            ops: Arc::new(RamDirInode::new()),
        });
        entries.insert(name.into(), inode.clone());
        Ok(inode)
    }

    fn create(&self, name: &str) -> Result<Arc<Inode>, VfsError> {
        let mut entries = self.entries.write();
        if entries.contains_key(name) {
            return Err(VfsError::AlreadyExists);
        }
        static FILE_INO: AtomicU64 = AtomicU64::new(2000);
        let ino = FILE_INO.fetch_add(1, Ordering::Relaxed);

        let inode = Arc::new(Inode {
            ino,
            inode_type: InodeType::File,
            ops: Arc::new(RamFileInode::new()),
        });
        entries.insert(name.into(), inode.clone());
        Ok(inode)
    }

    fn unlink(&self, name: &str) -> Result<(), VfsError> {
        let mut entries = self.entries.write();
        let target = entries.get(name).ok_or(VfsError::NotFound)?;
        if target.inode_type == InodeType::Directory {
            return Err(VfsError::IsDirectory);
        }
        entries.remove(name);
        Ok(())
    }

    fn rmdir(&self, name: &str) -> Result<(), VfsError> {
        let mut entries = self.entries.write();
        let target = entries.get(name).ok_or(VfsError::NotFound)?;
        if target.inode_type != InodeType::Directory {
            return Err(VfsError::NotDirectory);
        }
        entries.remove(name);
        Ok(())
    }

    fn stat(&self) -> Result<crate::fs::vfs::types::Stat, VfsError> {
        let entries = self.entries.read();
        Ok(crate::fs::vfs::types::Stat {
            size: entries.len() as u64,
            mode: 0o040755,
            nlink: 2,
            ..Default::default()
        })
    }

    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        Err(VfsError::NotFile)
    }
}

/// In-memory filesystem. Used as the root filesystem.
pub struct RamFs;

impl RamFs {
    /// Initialize root RamFS and create default mountpoints.
    pub fn init() -> Result<(), &'static str> {
        log::info!("[RamFS] Initializing Root RamFS...");
        let ramfs = RamFs;
        crate::fs::vfs::mount::MOUNT_TABLE
            .write()
            .mount("/", &ramfs)
            .map_err(|_| "Failed to mount RamFS root")?;

        let _ = crate::fs::vfs::path::mkdir("/dev");
        let _ = crate::fs::vfs::path::mkdir("/proc");
        let _ = crate::fs::vfs::path::mkdir("/mnt");

        log::info!("[RamFS] Root RamFS mounted at /.");
        Ok(())
    }
}

impl FileSystem for RamFs {
    fn name(&self) -> &'static str {
        "ramfs"
    }

    fn mount(&self) -> Result<SuperBlock, VfsError> {
        let next_ino = AtomicU64::new(1);
        let ino = next_ino.fetch_add(1, Ordering::Relaxed);

        let root_inode = Arc::new(Inode {
            ino,
            inode_type: InodeType::Directory,
            ops: Arc::new(RamDirInode::new()),
        });

        Ok(SuperBlock {
            fs_name: "ramfs",
            root_inode,
            next_ino,
            read_only: false,
        })
    }
}

crate::fs_initcall!(RamFs::init);
crate::MODULE_LICENSE!("BSD-2-Clause");
crate::MODULE_AUTHOR!("PetraOS Development Team");
crate::MODULE_DESCRIPTION!("In-Memory RamFS Root Filesystem");
