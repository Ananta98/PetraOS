use alloc::sync::Arc;
use crate::fs::vfs::types::{FileOps, VfsError};

/// Stub implementation for inotify file descriptor logic.
pub struct InotifyFd;

impl InotifyFd {
    pub fn new() -> Self {
        Self
    }
}

impl FileOps for InotifyFd {
    fn read(&self, _offset: usize, _buf: &mut [u8]) -> Result<usize, VfsError> {
        Err(VfsError::NotSupported)
    }

    fn write(&self, _offset: usize, _buf: &[u8]) -> Result<usize, VfsError> {
        Err(VfsError::NotSupported)
    }
}
