use crate::device::DriverError;
use crate::net::socket::Socket;
use crate::sync::Mutex;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

// ===== Open Flags =====

/// Open file for reading only.
pub const O_RDONLY: u32 = 0;
/// Open file for writing only.
pub const O_WRONLY: u32 = 1;
/// Open file for reading and writing.
pub const O_RDWR: u32 = 2;
/// Create the file if it does not exist.
pub const O_CREAT: u32 = 0x40;
/// Exclusive create flag (fail if file exists).
pub const O_EXCL: u32 = 0x80;
/// Truncate file to zero length on open.
pub const O_TRUNC: u32 = 0x200;
/// Append writes to the end of the file.
pub const O_APPEND: u32 = 0x400;
/// Non-blocking I/O (do not block on read/write).
pub const O_NONBLOCK: u32 = 0x800;
/// Fail if path is not a directory.
pub const O_DIRECTORY: u32 = 0x10000;
/// Do not follow the final symlink component.
pub const O_NOFOLLOW: u32 = 0x20000;

/// Special dirfd value representing the current working directory.
pub const AT_FDCWD: i32 = -100;
/// Do not follow symbolic links.
pub const AT_SYMLINK_NOFOLLOW: i32 = 0x100;
/// Remove directory instead of unlinking file.
pub const AT_REMOVEDIR: i32 = 0x200;
/// Follow symbolic link.
pub const AT_SYMLINK_FOLLOW: i32 = 0x400;
/// Allow empty relative pathname.
pub const AT_EMPTY_PATH: i32 = 0x1000;

/// Mask to extract the access mode (read/write) from flags.
const O_ACCMODE: u32 = 3;

/// Mask of the file type bits within a full stat mode (S_IFMT).
pub const MODE_TYPE_BITS: u32 = 0o170000;
/// Mask of the permission, set-user/group-id and sticky bits within a full stat mode.
pub const MODE_PERM_BITS: u32 = 0o7777;

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
    /// Directory not empty.
    NotEmpty,
    /// Is a directory.
    IsDirectory,
    /// Operation interrupted by a signal (EINTR).
    Interrupted,
    /// Symlink resolution depth exceeded the maximum (ELOOP).
    TooManySymlinks,
    /// Resource temporarily unavailable / operation would block (EAGAIN/EWOULDBLOCK).
    WouldBlock,
    /// An underlying device driver error occurred.
    DriverError(DriverError),
}

impl From<DriverError> for VfsError {
    fn from(e: DriverError) -> Self {
        VfsError::DriverError(e)
    }
}

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

// ===== Stat Structures =====

/// POSIX file metadata structure (kernel-internal).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stat {
    pub ino: u64,
    pub mode: u32,
    pub nlink: u32,
    pub uid: u32,
    pub gid: u32,
    pub size: u64,
    pub atime: u64,
    pub mtime: u64,
    pub ctime: u64,
    pub blksize: u64,
    pub blocks: u64,
}

/// `struct stat` as defined by the Linux ABI (x86-64 `statx`-compatible layout).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LinuxStat {
    pub st_dev: u64,
    pub st_ino: u64,
    pub st_nlink: u64,
    pub st_mode: u32,
    pub st_uid: u32,
    pub st_gid: u32,
    pub __pad0: u32,
    pub st_rdev: u64,
    pub st_size: i64,
    pub st_blksize: i64,
    pub st_blocks: i64,
    pub st_atime: u64,
    pub st_atime_nsec: u64,
    pub st_mtime: u64,
    pub st_mtime_nsec: u64,
    pub st_ctime: u64,
    pub st_ctime_nsec: u64,
    pub __glibc_reserved: [i64; 3],
}

/// Linux `struct statfs` (x86_64 ABI layout).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StatFs {
    pub f_type: i64,
    pub f_bsize: i64,
    pub f_blocks: u64,
    pub f_bfree: u64,
    pub f_bavail: u64,
    pub f_files: u64,
    pub f_ffree: u64,
    pub f_fsid: [i32; 2],
    pub f_namelen: i64,
    pub f_frsize: i64,
    pub f_flags: i64,
    pub f_spare: [i64; 4],
}

// ===== Seek =====

/// Seek directive for `lseek`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeekWhence {
    /// Seek to absolute offset from start of file.
    Set,
    /// Seek relative to the current file offset.
    Cur,
    /// Seek relative to end of file.
    End,
}

// ===== Inode Types =====

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
    /// Symbolic link.
    Symlink,
}

// ===== InodeOps Trait =====

/// Operations dispatch table for an inode.
///
/// Default implementations return appropriate errors so that concrete
/// filesystems only need to implement the operations they support.
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

    /// Remove a file entry from a directory inode.
    fn unlink(&self, _name: &str) -> Result<(), VfsError> {
        Err(VfsError::NotSupported)
    }

    /// Remove an empty subdirectory entry from a directory inode.
    fn rmdir(&self, _name: &str) -> Result<(), VfsError> {
        Err(VfsError::NotSupported)
    }

    /// Create a symbolic link entry in a directory inode.
    fn symlink(&self, _name: &str, _target: &str) -> Result<Arc<Inode>, VfsError> {
        Err(VfsError::NotSupported)
    }

    /// Read the target path from a symbolic link inode.
    fn readlink(&self) -> Result<String, VfsError> {
        Err(VfsError::NotSupported)
    }

    /// Create a hard link to `target_inode`.
    fn link(&self, _name: &str, _target_inode: &Arc<Inode>) -> Result<(), VfsError> {
        Err(VfsError::NotSupported)
    }

    /// Rename an entry from `old_name` to `new_name` in `new_dir`.
    fn rename(
        &self,
        _old_name: &str,
        _new_dir: &Arc<Inode>,
        _new_name: &str,
    ) -> Result<(), VfsError> {
        Err(VfsError::NotSupported)
    }

    /// List child entry names within a directory inode.
    fn readdir(&self) -> Result<Vec<String>, VfsError> {
        Err(VfsError::NotDirectory)
    }

    /// Fetch metadata stat structure.
    fn stat(&self) -> Result<Stat, VfsError> {
        Err(VfsError::NotSupported)
    }

    /// Truncate file to the specified size in bytes.
    fn truncate(&self, _size: usize) -> Result<(), VfsError> {
        Err(VfsError::NotSupported)
    }

    /// Change permissions mode of this inode.
    fn chmod(&self, _mode: u32) -> Result<(), VfsError> {
        Err(VfsError::NotSupported)
    }

    /// Change ownership (uid, gid) of this inode.
    fn chown(&self, _uid: u32, _gid: u32) -> Result<(), VfsError> {
        Err(VfsError::NotSupported)
    }

    /// Update access and modification timestamps.
    fn utimens(&self, _atime: u64, _mtime: u64) -> Result<(), VfsError> {
        Ok(())
    }

    /// Produce per-open-file I/O operations for this inode.
    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        Err(VfsError::NotSupported)
    }
}

// ===== Inode =====

/// An in-memory inode representing a single filesystem object.
pub struct Inode {
    /// Inode number, unique within the containing filesystem.
    pub ino: u64,
    /// The type of this inode (file, directory, char device, etc.).
    pub inode_type: InodeType,
    /// Operations dispatch table for this inode.
    pub ops: Arc<dyn InodeOps>,
}

// ===== FileOps Trait =====

/// Per-open-file I/O operations.
///
/// Obtained from `InodeOps::open()` and stored inside an open [`File`] description.
pub trait FileOps: Send + Sync {
    /// Read up to `buf.len()` bytes starting at `offset`.
    fn read(&self, _offset: usize, _buf: &mut [u8]) -> Result<usize, VfsError> {
        Err(VfsError::NotSupported)
    }

    /// Read up to `buf.len()` bytes starting at `offset` with open/fcntl flags (e.g. O_NONBLOCK).
    fn read_with_flags(
        &self,
        offset: usize,
        buf: &mut [u8],
        _flags: u32,
    ) -> Result<usize, VfsError> {
        self.read(offset, buf)
    }

    /// Write up to `buf.len()` bytes starting at `offset`.
    fn write(&self, _offset: usize, _buf: &[u8]) -> Result<usize, VfsError> {
        Err(VfsError::NotSupported)
    }

    /// Write up to `buf.len()` bytes starting at `offset` with open/fcntl flags.
    fn write_with_flags(
        &self,
        offset: usize,
        buf: &[u8],
        _flags: u32,
    ) -> Result<usize, VfsError> {
        self.write(offset, buf)
    }

    /// Seek to the specified offset based on `whence`.
    fn lseek(&self, _offset: i64, _whence: SeekWhence) -> Result<usize, VfsError> {
        Err(VfsError::NotSupported)
    }

    /// Truncate the file to `size` bytes.
    fn truncate(&self, _size: usize) -> Result<(), VfsError> {
        Err(VfsError::NotSupported)
    }

    /// Fetch file stat metadata.
    fn stat(&self) -> Result<Stat, VfsError> {
        Err(VfsError::NotSupported)
    }

    /// Change permissions mode of this file.
    fn chmod(&self, _mode: u32) -> Result<(), VfsError> {
        Err(VfsError::NotSupported)
    }

    /// Change ownership (uid, gid) of this file.
    fn chown(&self, _uid: u32, _gid: u32) -> Result<(), VfsError> {
        Err(VfsError::NotSupported)
    }

    /// Update access and modification timestamps.
    fn utimens(&self, _atime: u64, _mtime: u64) -> Result<(), VfsError> {
        Ok(())
    }

    /// Synchronize in-memory changes to persistent storage.
    fn sync(&self) -> Result<(), VfsError> {
        Ok(())
    }

    /// Perform device-specific control operations (ioctl).
    fn ioctl(&self, _cmd: u64, _arg: usize) -> Result<usize, VfsError> {
        Err(VfsError::NotSupported)
    }

    /// Return `true` if this file descriptor refers to a terminal (TTY).
    fn isatty(&self) -> bool {
        false
    }

    /// Return ready events bitmask matching `events` (POLLIN, POLLOUT, etc.).
    fn poll_events(&self, events: i16) -> i16 {
        events
    }

    /// Cast to Socket handle if this file is a socket.
    fn as_socket(&self) -> Option<Arc<Mutex<Socket>>> {
        None
    }
}

// ===== FileSystem Trait =====

/// Trait implemented by each filesystem type (ramfs, devfs, ext2, etc.).
pub trait FileSystem: Send + Sync {
    /// Human-readable filesystem name (e.g., "ramfs", "devfs", "ext2").
    fn name(&self) -> &'static str;

    /// Create a fresh superblock and root inode for a new mount instance.
    fn mount(&self) -> Result<SuperBlock, VfsError>;
}

// ===== SuperBlock =====

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
