//! Kernel Message Log Device (/dev/kmsg)
//!
//! Provides userspace access to write to and read from the kernel logger buffer.

use alloc::sync::Arc;
use crate::fs::vfs::types::{FileOps, InodeOps, Stat, VfsError};

/// Inode for the `/dev/kmsg` device.
pub struct KmsgInode;

impl InodeOps for KmsgInode {
    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        Ok(Arc::new(KmsgFileOps))
    }

    fn stat(&self) -> Result<Stat, VfsError> {
        Ok(Stat {
            mode: 0o020666, // S_IFCHR | 0666
            nlink: 1,
            ..Default::default()
        })
    }
}

/// File operations for `/dev/kmsg`.
pub struct KmsgFileOps;

impl FileOps for KmsgFileOps {
    fn read(&self, _offset: usize, _buf: &mut [u8]) -> Result<usize, VfsError> {
        Ok(0)
    }

    fn write(&self, _offset: usize, buf: &[u8]) -> Result<usize, VfsError> {
        if let Ok(s) = core::str::from_utf8(buf) {
            let trimmed = s.trim_end_matches(&['\r', '\n'][..]);
            log::info!("[kmsg] {}", trimmed);
        } else {
            log::info!("[kmsg] <binary data len={}>", buf.len());
        }
        Ok(buf.len())
    }
}
