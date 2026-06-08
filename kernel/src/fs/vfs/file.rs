use alloc::sync::Arc;
use crate::sync::spinlock::Spinlock;
use crate::fs::errno::VfsError;
use crate::fs::flags;
use super::dentry::Dentry;

/// Per-open-file I/O operations.
///
/// Each filesystem provides its own `FileOps` implementation via
/// [`InodeOps::open()`](super::inode::InodeOps::open). This allows different
/// behaviour per inode type (e.g., ramfs file vs. console device).
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
        if !flags::can_read(self.flags) {
            return Err(VfsError::PermissionDenied);
        }
        let mut offset = self.offset.lock();
        let bytes_read = self.ops.read(*offset, buf)?;
        *offset += bytes_read;
        Ok(bytes_read)
    }

    /// Write to the file, advancing the offset. Enforces `O_WRONLY`/`O_RDWR`.
    pub fn write(&self, buf: &[u8]) -> Result<usize, VfsError> {
        if !flags::can_write(self.flags) {
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
