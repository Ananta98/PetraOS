//! Full Character Device (/dev/full)
//!
//! Provides the standard UNIX /dev/full device:
//! * Reads from /dev/full return zero bytes (like /dev/zero).
//! * Writes to /dev/full always return ENOSPC (NoSpace) error.

use crate::fs::vfs::types::{FileOps, InodeOps, Stat, VfsError};
use alloc::sync::Arc;

/// Inode for the `/dev/full` device.
pub struct FullInode;

impl InodeOps for FullInode {
    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        Ok(Arc::new(FullFileOps))
    }

    fn stat(&self) -> Result<Stat, VfsError> {
        Ok(Stat {
            mode: 0o020666, // S_IFCHR | 0666
            nlink: 1,
            ..Default::default()
        })
    }
}

/// File operations for `/dev/full`.
pub struct FullFileOps;

impl FileOps for FullFileOps {
    fn read(&self, _offset: usize, buf: &mut [u8]) -> Result<usize, VfsError> {
        buf.fill(0);
        Ok(buf.len())
    }

    fn write(&self, _offset: usize, _buf: &[u8]) -> Result<usize, VfsError> {
        Err(VfsError::NoSpace)
    }
}
