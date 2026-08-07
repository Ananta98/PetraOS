use alloc::sync::Arc;
use crate::fs::vfs::types::{FileOps, VfsError};

/// Stub implementation for signalfd file descriptor logic.
pub struct SignalFd;

impl SignalFd {
    pub fn new() -> Self {
        Self
    }
}

impl FileOps for SignalFd {
    fn read(&self, _offset: usize, _buf: &mut [u8]) -> Result<usize, VfsError> {
        Err(VfsError::NotSupported)
    }

    fn write(&self, _offset: usize, _buf: &[u8]) -> Result<usize, VfsError> {
        Err(VfsError::NotSupported)
    }
}
