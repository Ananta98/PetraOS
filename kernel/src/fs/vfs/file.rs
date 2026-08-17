use super::dentry::Dentry;
use super::types::{FileOps, SeekWhence, VfsError, can_read, can_write};
use crate::sync::spinlock::Spinlock;
use alloc::sync::Arc;

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

    /// POSIX lseek implementation.
    pub fn lseek(&self, offset: i64, whence: SeekWhence) -> Result<usize, VfsError> {
        if let Ok(new_pos) = self.ops.lseek(offset, whence) {
            let mut off = self.offset.lock();
            *off = new_pos;
            return Ok(new_pos);
        }

        let current_pos = *self.offset.lock();
        let base = match whence {
            SeekWhence::Set => 0i64,
            SeekWhence::Cur => current_pos as i64,
            SeekWhence::End => {
                let stat = self.ops.stat().or_else(|_| self.dentry.inode.ops.stat())?;
                stat.size as i64
            }
        };

        let target = base + offset;
        if target < 0 {
            return Err(VfsError::InvalidInput);
        }

        let new_offset = target as usize;
        let mut off = self.offset.lock();
        *off = new_offset;
        Ok(new_offset)
    }
}
