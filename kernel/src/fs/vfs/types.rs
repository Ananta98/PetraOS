use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use ostd::Error;
use ostd::sync::SpinLock;

/// Common result type for VFS operations.
pub type Result<T> = core::result::Result<T, Error>;

/// Unix-compatible file types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    Regular,
    Directory,
    Symlink,
    CharDevice,
    BlockDevice,
}

/// Metadata attributes of an inode, analogous to `stat` in Unix.
#[derive(Debug, Clone)]
pub struct Metadata {
    pub size: usize,
    pub file_type: FileType,
    pub mode: u32,
    pub inode_num: u64,
    pub nlink: u32,
}

/// Directory entry returned during `readdir` operations.
#[derive(Debug, Clone)]
pub struct DirEntry {
    pub name: String,
    pub inode_num: u64,
    pub file_type: FileType,
}

/// Seek positions for file descriptors.
#[derive(Debug, Clone, Copy)]
pub enum SeekFrom {
    Start(usize),
    Current(isize),
    End(isize),
}

/// Interface for inode operations, analogous to `inode_operations` in Linux.
pub trait InodeOps: Send + Sync {
    /// Look up a child inode by name.
    fn lookup(&self, name: &str) -> Result<Arc<dyn InodeOps>>;

    /// Create a regular file under this directory.
    fn create(&self, name: &str, mode: u32) -> Result<Arc<dyn InodeOps>>;

    /// Create a directory under this directory.
    fn mkdir(&self, name: &str, mode: u32) -> Result<Arc<dyn InodeOps>>;

    /// Create a symbolic link under this directory pointing to `target`.
    fn symlink(&self, name: &str, target: &str) -> Result<Arc<dyn InodeOps>>;

    /// Get inode metadata (stat).
    fn metadata(&self) -> Result<Metadata>;

    /// Read symlink contents.
    fn read_link(&self) -> Result<String>;

    /// Open a file descriptor, producing file operations.
    fn open(&self, flags: u32) -> Result<Box<dyn FileOps>>;

    /// Get the dyn Any reference to the concrete type.
    fn as_any(&self) -> &dyn core::any::Any;

    /// Remove a directory entry by name.
    fn unlink(&self, name: &str) -> Result<()>;

    /// Rename a directory entry.
    fn rename(&self, old_name: &str, new_parent: &Arc<dyn InodeOps>, new_name: &str) -> Result<()>;
}

/// Interface for open file operations, analogous to `file_operations` in Linux.
pub trait FileOps: Send + Sync {
    /// Read bytes from the file starting at `offset`. Updates `offset`.
    fn read(&mut self, buf: &mut [u8], offset: &mut usize) -> Result<usize>;

    /// Write bytes to the file starting at `offset`. Updates `offset`.
    fn write(&mut self, buf: &[u8], offset: &mut usize) -> Result<usize>;

    /// Adjust the file offset.
    fn seek(&mut self, pos: SeekFrom, offset: &mut usize) -> Result<usize>;

    /// Read directory entries.
    fn readdir(&mut self) -> Result<Vec<DirEntry>>;

    /// Returns a reference to the underlying type as `Any`, if supported.
    fn as_any(&self) -> Option<&dyn core::any::Any> {
        None
    }
}

/// Directory entry in the VFS cache, forming the filesystem hierarchy.
pub struct Dentry {
    /// Name of this path component.
    pub name: String,
    /// Associated inode.
    pub inode: Arc<dyn InodeOps>,
    /// Parent dentry (weak ref to avoid reference cycles).
    pub parent: Option<Weak<Dentry>>,
    /// Child dentries cache.
    pub children: SpinLock<BTreeMap<String, Arc<Dentry>>>,
    /// Maps to a mounted filesystem superblock if this dentry is a mount point.
    pub mounted_sb: SpinLock<Option<Arc<SuperBlock>>>,
    /// Points to the mountpoint dentry in the host filesystem if this is a mounted root.
    pub mount_point: SpinLock<Option<Weak<Dentry>>>,
}

impl Dentry {
    /// Create a new dentry.
    pub fn new(name: &str, inode: Arc<dyn InodeOps>, parent: Option<Weak<Dentry>>) -> Arc<Self> {
        Arc::new(Self {
            name: String::from(name),
            inode,
            parent,
            children: SpinLock::new(BTreeMap::new()),
            mounted_sb: SpinLock::new(None),
            mount_point: SpinLock::new(None),
        })
    }
}

/// Represents a mounted filesystem instance.
pub struct SuperBlock {
    /// Name of the filesystem type (e.g. "tmpfs").
    pub fs_type: String,
    /// Root dentry of this mounted filesystem.
    pub root_dentry: SpinLock<Option<Arc<Dentry>>>,
}

/// Interface for a filesystem type driver.
pub trait FileSystem: Send + Sync {
    /// Name of the filesystem type (e.g., "tmpfs").
    fn name(&self) -> &'static str;
    /// Mount the filesystem.
    fn mount(&self, flags: u32, data: &[u8]) -> Result<Arc<SuperBlock>>;
}
