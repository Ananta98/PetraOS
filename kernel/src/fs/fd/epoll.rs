use alloc::sync::Arc;
use crate::fs::vfs::types::{FileOps, VfsError};

/// Stub implementation for epoll file descriptor logic.
pub struct EpollFd;

impl EpollFd {
    pub fn new() -> Self {
        Self
    }
}

impl FileOps for EpollFd {
    fn read(&self, _offset: usize, _buf: &mut [u8]) -> Result<usize, VfsError> {
        Err(VfsError::NotSupported)
    }

    fn write(&self, _offset: usize, _buf: &[u8]) -> Result<usize, VfsError> {
        Err(VfsError::NotSupported)
    }
}
