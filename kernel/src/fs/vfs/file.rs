use super::dentry::Dentry;
use super::types::{FileOps, SeekWhence, VfsError, can_read, can_write};
use crate::fs::vfs::types::InodeType;
use crate::sync::Mutex;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicU32, Ordering};

/// An open file description, tying a dentry to per-open state (offset, flags)
/// and the I/O operations obtained from the inode.
pub struct File {
    /// The dentry this file was opened from.
    pub dentry: Arc<Dentry>,
    /// Current read/write offset (protected by a mutex for safe concurrent access).
    pub offset: Mutex<usize>,
    /// Open flags (O_RDONLY, O_WRONLY, O_RDWR, etc.).
    pub flags: AtomicU32,
    /// Per-open I/O operations from the inode.
    pub ops: Arc<dyn FileOps>,
}

impl File {
    /// Create a new open file description.
    pub fn new(dentry: Arc<Dentry>, flags: u32, ops: Arc<dyn FileOps>) -> Self {
        Self {
            dentry,
            offset: Mutex::new(0),
            flags: AtomicU32::new(flags),
            ops,
        }
    }

    /// Return the current open flags.
    pub fn flags(&self) -> u32 {
        self.flags.load(Ordering::Acquire)
    }

    /// Update the open flags (e.g. via fcntl F_SETFL).
    pub fn set_flags(&self, flags: u32) {
        self.flags.store(flags, Ordering::Release);
    }

    /// Read from the file, advancing the offset. Enforces `O_RDONLY`/`O_RDWR`.
    pub fn read(&self, buf: &mut [u8]) -> Result<usize, VfsError> {
        let flags = self.flags();
        if !can_read(flags) {
            return Err(VfsError::PermissionDenied);
        }
        let mut offset = self.offset.lock();
        let bytes_read = self.ops.read_with_flags(*offset, buf, flags)?;
        *offset += bytes_read;
        Ok(bytes_read)
    }

    /// Write to the file, advancing the offset. Enforces `O_WRONLY`/`O_RDWR` and `O_APPEND`.
    pub fn write(&self, buf: &[u8]) -> Result<usize, VfsError> {
        let flags = self.flags();
        if !can_write(flags) {
            return Err(VfsError::PermissionDenied);
        }
        let mut offset = self.offset.lock();
        if (flags & super::types::O_APPEND) != 0 {
            if let Ok(stat) = self.ops.stat().or_else(|_| self.dentry.inode.ops.stat()) {
                *offset = stat.size as usize;
            }
        }
        let bytes_written = self.ops.write_with_flags(*offset, buf, flags)?;
        *offset += bytes_written;
        Ok(bytes_written)
    }

    /// Seek to an absolute offset directly (no `O_*` flag checks).
    pub fn seek(&self, new_offset: usize) {
        *self.offset.lock() = new_offset;
    }

    /// POSIX `lseek` implementation.
    ///
    /// Acquires the offset lock exactly once to avoid a double-lock in the
    /// fallback branch. If the underlying `FileOps` provides a custom `lseek`,
    /// that result is used directly; otherwise the offset is updated in-place.
    pub fn lseek(&self, offset: i64, whence: SeekWhence) -> Result<usize, VfsError> {
        if self.dentry.inode.inode_type != InodeType::File {
            return Err(VfsError::NotSupported);
        }

        // Delegate to FileOps if it provides a custom seek implementation.
        if let Ok(new_pos) = self.ops.lseek(offset, whence) {
            *self.offset.lock() = new_pos;
            return Ok(new_pos);
        }

        // Fallback: compute the new offset ourselves — single lock acquisition.
        let mut off = self.offset.lock();
        let base = match whence {
            SeekWhence::Set => 0i64,
            SeekWhence::Cur => *off as i64,
            SeekWhence::End => {
                let stat = self.ops.stat().or_else(|_| self.dentry.inode.ops.stat())?;
                stat.size as i64
            }
        };

        let target = base + offset;
        if target < 0 {
            return Err(VfsError::InvalidInput);
        }

        *off = target as usize;
        Ok(*off)
    }
}
