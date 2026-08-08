use crate::sync::spinlock::Spinlock;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

/// Unified error type for all VFS and filesystem operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsError {
    /// Entry not found during lookup.
    NotFound,
    /// Expected a directory but found a non-directory inode.
    NotDirectory,
    /// Expected a regular file but found a non-file inode.
    NotFile,
    /// Entry already exists (e.g., mkdir/create duplicate).
    AlreadyExists,
    /// Invalid argument or malformed input.
    InvalidInput,
    /// Operation not permitted (e.g., wrong open flags).
    PermissionDenied,
    /// Filesystem is mounted read-only.
    ReadOnlyFs,
    /// Operation not supported by this filesystem or inode type.
    NotSupported,
    /// Bad file descriptor (not open or already closed).
    BadFd,
}

/// Open file for reading only.
pub const O_RDONLY: u32 = 0;
/// Open file for writing only.
pub const O_WRONLY: u32 = 1;
/// Open file for reading and writing.
pub const O_RDWR: u32 = 2;
/// Create the file if it does not exist.
pub const O_CREAT: u32 = 0x40;

/// Mask to extract the access mode (read/write) from flags.
const O_ACCMODE: u32 = 3;

/// Returns `true` if the given flags permit reading.
pub fn can_read(flags: u32) -> bool {
    let mode = flags & O_ACCMODE;
    mode == O_RDONLY || mode == O_RDWR
}

/// Returns `true` if the given flags permit writing.
pub fn can_write(flags: u32) -> bool {
    let mode = flags & O_ACCMODE;
    mode == O_WRONLY || mode == O_RDWR
}

/// The kind of object an inode represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InodeType {
    /// Regular file.
    File,
    /// Directory.
    Directory,
    /// Character device (e.g., /dev/console).
    CharDevice,
    /// Block device (e.g., /dev/sda).
    BlockDevice,
    /// Symbolic link (reserved for future use).
    Symlink,
}

/// Operations dispatch table for an inode.
pub trait InodeOps: Send + Sync {
    /// Look up a child entry by name within a directory inode.
    fn lookup(&self, _name: &str) -> Result<Arc<Inode>, VfsError> {
        Err(VfsError::NotDirectory)
    }

    /// Create a new regular file child within a directory inode.
    fn create(&self, _name: &str) -> Result<Arc<Inode>, VfsError> {
        Err(VfsError::NotSupported)
    }

    /// Create a new subdirectory within a directory inode.
    fn mkdir(&self, _name: &str) -> Result<Arc<Inode>, VfsError> {
        Err(VfsError::NotSupported)
    }

    /// List child entry names within a directory inode.
    fn readdir(&self) -> Result<Vec<String>, VfsError> {
        Err(VfsError::NotDirectory)
    }

    /// Produce per-open-file I/O operations for this inode.
    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        Err(VfsError::NotSupported)
    }
}

/// An in-memory inode representing a single filesystem object.
pub struct Inode {
    /// Inode number, unique within the containing filesystem.
    pub ino: u64,
    /// The type of this inode (file, directory, char device, etc.).
    pub inode_type: InodeType,
    /// Operations dispatch table for this inode.
    pub ops: Arc<dyn InodeOps>,
}

/// Per-open-file I/O operations.
pub trait FileOps: Send + Sync {
    /// Read up to `buf.len()` bytes starting at `offset`.
    fn read(&self, _offset: usize, _buf: &mut [u8]) -> Result<usize, VfsError> {
        Err(VfsError::NotSupported)
    }

    /// Write up to `buf.len()` bytes starting at `offset`.
    fn write(&self, _offset: usize, _buf: &[u8]) -> Result<usize, VfsError> {
        Err(VfsError::NotSupported)
    }
}

use super::dcache::Dentry;

/// An open file description, tying a dentry to per-open state (offset, flags)
/// and the I/O operations obtained from the inode.
pub struct File {
    /// The dentry this file was opened from.
    pub dentry: Arc<Dentry>,
    /// Current read/write offset.
    pub offset: Spinlock<usize>,
    /// Open flags (O_RDONLY, O_WRONLY, O_RDWR, etc.).
    pub flags: u32,
    /// Per-open I/O operations from the inode.
    pub ops: Arc<dyn FileOps>,
}

impl File {
    /// Create a new open file description.
    pub fn new(dentry: Arc<Dentry>, flags: u32, ops: Arc<dyn FileOps>) -> Self {
        Self {
            dentry,
            offset: Spinlock::new(0),
            flags,
            ops,
        }
    }

    /// Read from the file, advancing the offset. Enforces `O_RDONLY`/`O_RDWR`.
    pub fn read(&self, buf: &mut [u8]) -> Result<usize, VfsError> {
        if !can_read(self.flags) {
            return Err(VfsError::PermissionDenied);
        }
        let mut offset = self.offset.lock();
        let bytes_read = self.ops.read(*offset, buf)?;
        *offset += bytes_read;
        Ok(bytes_read)
    }

    /// Write to the file, advancing the offset. Enforces `O_WRONLY`/`O_RDWR`.
    pub fn write(&self, buf: &[u8]) -> Result<usize, VfsError> {
        if !can_write(self.flags) {
            return Err(VfsError::PermissionDenied);
        }
        let mut offset = self.offset.lock();
        let bytes_written = self.ops.write(*offset, buf)?;
        *offset += bytes_written;
        Ok(bytes_written)
    }

    /// Seek to an absolute offset.
    pub fn seek(&self, new_offset: usize) {
        let mut offset = self.offset.lock();
        *offset = new_offset;
    }
}

/// Trait implemented by each filesystem type (ramfs, devfs, procfs, ext2, etc.).
pub trait FileSystem: Send + Sync {
    /// Human-readable name (e.g., "ramfs", "devfs", "procfs").
    fn name(&self) -> &'static str;

    /// Create a fresh superblock and root inode for a new mount instance.
    fn mount(&self) -> Result<SuperBlock, VfsError>;
}

/// Per-mount metadata. Each mounted filesystem instance has exactly one `SuperBlock`.
pub struct SuperBlock {
    /// Name of the filesystem type that created this superblock.
    pub fs_name: &'static str,
    /// Root inode of this filesystem instance.
    pub root_inode: Arc<Inode>,
    /// Monotonically increasing inode number allocator for this fs instance.
    pub next_ino: AtomicU64,
    /// If `true`, write operations are rejected with `VfsError::ReadOnlyFs`.
    pub read_only: bool,
}

impl SuperBlock {
    /// Allocate the next unique inode number within this filesystem instance.
    pub fn alloc_ino(&self) -> u64 {
        self.next_ino.fetch_add(1, Ordering::Relaxed)
    }
}
