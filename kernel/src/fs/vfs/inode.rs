use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::string::String;
use crate::fs::errno::VfsError;
use super::file::FileOps;

/// The kind of object an inode represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InodeType {
    /// Regular file.
    File,
    /// Directory.
    Directory,
    /// Character device (e.g., /dev/console).
    CharDevice,
    /// Symbolic link (reserved for future use).
    Symlink,
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

/// Operations on an inode.
///
/// Directory operations (lookup, create, mkdir, readdir) are handled here.
/// I/O operations are handled by [`FileOps`], obtained via [`InodeOps::open`].
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
    ///
    /// Called during `open()` to obtain the [`FileOps`] that will handle
    /// read/write/seek on the resulting file description.
    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        Err(VfsError::NotSupported)
    }
}
