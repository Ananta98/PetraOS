use crate::drivers::time::cmos_rtc;
use crate::fs::vfs::types::{
    FileOps, FileSystem, MODE_PERM_BITS, MODE_TYPE_BITS, Stat, SuperBlock, VfsError,
};
use crate::fs::{Inode, InodeOps, InodeType};
use crate::sync::Mutex;
use crate::sync::rwlock::RwLock;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

// ===== InodeMetadata — mutable inode metadata =====

/// Mutable ownership, permission and timestamp metadata for an in-memory
/// inode. Shared between an inode's `InodeOps` and its per-open `FileOps`
/// so that changes made through either path are mutually visible.
#[derive(Debug, Clone, Copy)]
pub struct InodeMetadata {
    /// Full mode including the file type bits (e.g. `0o100644`).
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub atime: u64,
    pub mtime: u64,
    pub ctime: u64,
}

impl InodeMetadata {
    /// Create default metadata carrying the given full mode.
    fn new(mode: u32) -> Self {
        Self {
            mode,
            uid: 0,
            gid: 0,
            atime: 0,
            mtime: 0,
            ctime: 0,
        }
    }
}

/// Shared handle to mutable inode metadata.
type SharedMetadata = Arc<Mutex<InodeMetadata>>;

/// Current wall-clock time in seconds since the UNIX epoch.
fn now_secs() -> u64 {
    cmos_rtc::get_wall_time().0
}

/// Change permission bits of shared metadata, preserving the file type bits.
fn apply_chmod(meta: &SharedMetadata, mode: u32) -> Result<(), VfsError> {
    let mut metadata = meta.lock();
    metadata.mode = (metadata.mode & MODE_TYPE_BITS) | (mode & MODE_PERM_BITS);
    metadata.ctime = now_secs();
    Ok(())
}

/// Change owner/group of shared metadata.
fn apply_chown(meta: &SharedMetadata, uid: u32, gid: u32) -> Result<(), VfsError> {
    let mut metadata = meta.lock();
    metadata.uid = uid;
    metadata.gid = gid;
    metadata.ctime = now_secs();
    Ok(())
}

/// Update access/modification timestamps of shared metadata.
fn apply_utimens(meta: &SharedMetadata, atime: u64, mtime: u64) -> Result<(), VfsError> {
    let mut metadata = meta.lock();
    metadata.atime = atime;
    metadata.mtime = mtime;
    metadata.ctime = now_secs();
    Ok(())
}

/// Build a [`Stat`] from shared metadata plus the caller-supplied size/link/block info.
fn metadata_stat(
    meta: &SharedMetadata,
    size: u64,
    nlink: u32,
    blocks: u64,
) -> Result<Stat, VfsError> {
    let metadata = meta.lock();
    Ok(Stat {
        mode: metadata.mode,
        nlink,
        uid: metadata.uid,
        gid: metadata.gid,
        size,
        atime: metadata.atime,
        mtime: metadata.mtime,
        ctime: metadata.ctime,
        blocks,
        blksize: 4096,
        ..Default::default()
    })
}

// ===== RamFileOps — in-memory file I/O =====

/// File I/O operations for an in-memory regular file.
pub struct RamFileOps {
    pub content: Arc<RwLock<Vec<u8>>>,
    pub meta: SharedMetadata,
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
        self.content.write().resize(size, 0);
        Ok(())
    }

    fn chmod(&self, mode: u32) -> Result<(), VfsError> {
        apply_chmod(&self.meta, mode)
    }

    fn chown(&self, uid: u32, gid: u32) -> Result<(), VfsError> {
        apply_chown(&self.meta, uid, gid)
    }

    fn utimens(&self, atime: u64, mtime: u64) -> Result<(), VfsError> {
        apply_utimens(&self.meta, atime, mtime)
    }

    fn stat(&self) -> Result<Stat, VfsError> {
        let size = self.content.read().len() as u64;
        metadata_stat(&self.meta, size, 1, (size + 511) / 512)
    }
}

// ===== RamFileInode — in-memory regular file inode =====

/// Inode operations for an in-memory regular file.
pub struct RamFileInode {
    pub content: Arc<RwLock<Vec<u8>>>,
    pub meta: SharedMetadata,
}

impl RamFileInode {
    pub fn new() -> Self {
        Self {
            content: Arc::new(RwLock::new(Vec::new())),
            meta: Arc::new(Mutex::new(InodeMetadata::new(0o100644))),
        }
    }
}

impl InodeOps for RamFileInode {
    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        Ok(Arc::new(RamFileOps {
            content: self.content.clone(),
            meta: self.meta.clone(),
        }))
    }

    fn stat(&self) -> Result<Stat, VfsError> {
        let size = self.content.read().len() as u64;
        metadata_stat(&self.meta, size, 1, (size + 511) / 512)
    }

    fn truncate(&self, size: usize) -> Result<(), VfsError> {
        self.content.write().resize(size, 0);
        Ok(())
    }

    fn chmod(&self, mode: u32) -> Result<(), VfsError> {
        apply_chmod(&self.meta, mode)
    }

    fn chown(&self, uid: u32, gid: u32) -> Result<(), VfsError> {
        apply_chown(&self.meta, uid, gid)
    }

    fn utimens(&self, atime: u64, mtime: u64) -> Result<(), VfsError> {
        apply_utimens(&self.meta, atime, mtime)
    }
}

// ===== RamSymlinkInode — in-memory symbolic link inode =====

/// Inode operations for an in-memory symbolic link.
pub struct RamSymlinkInode {
    pub target: String,
    pub meta: SharedMetadata,
}

impl RamSymlinkInode {
    pub fn new(target: String) -> Self {
        Self {
            target,
            meta: Arc::new(Mutex::new(InodeMetadata::new(0o120777))),
        }
    }
}

impl InodeOps for RamSymlinkInode {
    fn readlink(&self) -> Result<String, VfsError> {
        Ok(self.target.clone())
    }

    fn stat(&self) -> Result<Stat, VfsError> {
        metadata_stat(&self.meta, self.target.len() as u64, 1, 0)
    }

    fn chmod(&self, mode: u32) -> Result<(), VfsError> {
        apply_chmod(&self.meta, mode)
    }

    fn chown(&self, uid: u32, gid: u32) -> Result<(), VfsError> {
        apply_chown(&self.meta, uid, gid)
    }

    fn utimens(&self, atime: u64, mtime: u64) -> Result<(), VfsError> {
        apply_utimens(&self.meta, atime, mtime)
    }
}

// ===== RamDirInode — in-memory directory inode =====

/// Inode operations for an in-memory directory.
///
/// Uses a shared `Arc<AtomicU64>` inode counter from the filesystem's
/// `SuperBlock::next_ino` so that all inode types draw from the same
/// monotonically increasing sequence, preventing collisions.
pub struct RamDirInode {
    pub entries: RwLock<BTreeMap<String, Arc<Inode>>>,
    /// Metadata of this directory inode itself.
    pub meta: SharedMetadata,
    /// Shared inode number allocator from the mounted SuperBlock.
    next_ino: Arc<AtomicU64>,
    /// Inode number of this directory.
    pub ino: u64,
}

impl RamDirInode {
    /// Create a new directory inode with a shared inode counter.
    pub fn new(ino: u64, next_ino: Arc<AtomicU64>) -> Self {
        Self {
            entries: RwLock::new(BTreeMap::new()),
            meta: Arc::new(Mutex::new(InodeMetadata::new(0o040755))),
            next_ino,
            ino,
        }
    }

    /// Allocate the next unique inode number.
    fn alloc_ino(&self) -> u64 {
        self.next_ino.fetch_add(1, Ordering::Relaxed)
    }

    /// Register a device inode directly (used by devfs bridge).
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
        self.entries
            .read()
            .get(name)
            .cloned()
            .ok_or(VfsError::NotFound)
    }

    fn readdir(&self) -> Result<Vec<String>, VfsError> {
        Ok(self.entries.read().keys().cloned().collect())
    }

    fn mkdir(&self, name: &str) -> Result<Arc<Inode>, VfsError> {
        let mut entries = self.entries.write();
        if entries.contains_key(name) {
            return Err(VfsError::AlreadyExists);
        }
        let ino = self.alloc_ino();
        let inode = Arc::new(Inode {
            ino,
            inode_type: InodeType::Directory,
            ops: Arc::new(RamDirInode::new(ino, self.next_ino.clone())),
        });
        entries.insert(name.into(), inode.clone());
        Ok(inode)
    }

    fn create(&self, name: &str) -> Result<Arc<Inode>, VfsError> {
        let mut entries = self.entries.write();
        if entries.contains_key(name) {
            return Err(VfsError::AlreadyExists);
        }
        let ino = self.alloc_ino();
        let inode = Arc::new(Inode {
            ino,
            inode_type: InodeType::File,
            ops: Arc::new(RamFileInode::new()),
        });
        entries.insert(name.into(), inode.clone());
        Ok(inode)
    }

    fn link(&self, name: &str, target_inode: &Arc<Inode>) -> Result<(), VfsError> {
        let mut entries = self.entries.write();
        if entries.contains_key(name) {
            return Err(VfsError::AlreadyExists);
        }
        entries.insert(name.into(), target_inode.clone());
        Ok(())
    }

    fn symlink(&self, name: &str, target: &str) -> Result<Arc<Inode>, VfsError> {
        let mut entries = self.entries.write();
        if entries.contains_key(name) {
            return Err(VfsError::AlreadyExists);
        }
        let ino = self.alloc_ino();
        let inode = Arc::new(Inode {
            ino,
            inode_type: InodeType::Symlink,
            ops: Arc::new(RamSymlinkInode::new(target.into())),
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

    fn rename(
        &self,
        old_name: &str,
        new_dir: &Arc<Inode>,
        new_name: &str,
    ) -> Result<(), VfsError> {
        if self.ino == new_dir.ino {
            if old_name == new_name {
                return Ok(());
            }
            let mut entries = self.entries.write();
            let inode = entries.remove(old_name).ok_or(VfsError::NotFound)?;
            entries.insert(new_name.into(), inode);
            Ok(())
        } else {
            let inode = {
                let mut entries = self.entries.write();
                entries.remove(old_name).ok_or(VfsError::NotFound)?
            };
            if let Ok(existing) = new_dir.ops.lookup(new_name) {
                if existing.inode_type == InodeType::Directory {
                    self.entries.write().insert(old_name.into(), inode);
                    return Err(VfsError::IsDirectory);
                }
                let _ = new_dir.ops.unlink(new_name);
            }
            if let Err(e) = new_dir.ops.link(new_name, &inode) {
                self.entries.write().insert(old_name.into(), inode);
                return Err(e);
            }
            Ok(())
        }
    }

    fn chmod(&self, mode: u32) -> Result<(), VfsError> {
        apply_chmod(&self.meta, mode)
    }

    fn chown(&self, uid: u32, gid: u32) -> Result<(), VfsError> {
        apply_chown(&self.meta, uid, gid)
    }

    fn utimens(&self, atime: u64, mtime: u64) -> Result<(), VfsError> {
        apply_utimens(&self.meta, atime, mtime)
    }

    fn stat(&self) -> Result<Stat, VfsError> {
        let entry_count = self.entries.read().len() as u64;
        metadata_stat(&self.meta, entry_count, 2, 1)
    }

    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        Ok(Arc::new(RamDirFileOps))
    }
}

// ===== RamDirFileOps — file ops for directories =====

pub struct RamDirFileOps;

impl FileOps for RamDirFileOps {
    fn read(&self, _offset: usize, _buf: &mut [u8]) -> Result<usize, VfsError> {
        Err(VfsError::IsDirectory)
    }

    fn write(&self, _offset: usize, _buf: &[u8]) -> Result<usize, VfsError> {
        Err(VfsError::IsDirectory)
    }

    fn stat(&self) -> Result<crate::fs::vfs::types::Stat, VfsError> {
        Ok(crate::fs::vfs::types::Stat {
            size: 0,
            mode: 0o040755,
            nlink: 2,
            ..Default::default()
        })
    }
}

// ===== RamFs — in-memory filesystem =====

/// In-memory filesystem used as the root filesystem.
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
        let next_ino = Arc::new(AtomicU64::new(1));
        let ino = next_ino.fetch_add(1, Ordering::Relaxed);

        let root_inode = Arc::new(Inode {
            ino,
            inode_type: InodeType::Directory,
            ops: Arc::new(RamDirInode::new(ino, next_ino.clone())),
        });

        Ok(SuperBlock {
            fs_name: "ramfs",
            root_inode,
            // The SuperBlock also holds a copy of the counter via AtomicU64.
            // Wrap the Arc value — both the root dir and the superblock share the same counter.
            next_ino: AtomicU64::new(next_ino.load(Ordering::Relaxed)),
            read_only: false,
        })
    }
}

crate::core_initcall!(RamFs::init);
crate::MODULE_LICENSE!("BSD-2-Clause");
crate::MODULE_AUTHOR!("PetraOS Development Team");
crate::MODULE_DESCRIPTION!("In-Memory RamFS Root Filesystem");
