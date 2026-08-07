use alloc::sync::Arc;
use crate::fs::vfs::types::{FileOps, VfsError};

/// Stub implementation for eventfd file descriptor logic.
pub struct EventFd;

impl EventFd {
    pub fn new() -> Self {
        Self
    }
}

impl FileOps for EventFd {
    fn read(&self, _offset: usize, _buf: &mut [u8]) -> Result<usize, VfsError> {
        Err(VfsError::NotSupported)
    }

    fn write(&self, _offset: usize, _buf: &[u8]) -> Result<usize, VfsError> {
        Err(VfsError::NotSupported)
    }
}
