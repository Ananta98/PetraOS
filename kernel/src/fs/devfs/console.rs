use alloc::sync::Arc;
use crate::fs::errno::VfsError;
use crate::fs::vfs::inode::InodeOps;
use crate::fs::vfs::file::FileOps;

/// Inode for the `/dev/console` device.
///
/// Writing to this device logs output via the kernel's serial/log interface.
/// Reading returns EOF (0 bytes).
pub struct ConsoleInode;

impl InodeOps for ConsoleInode {
    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        Ok(Arc::new(ConsoleFileOps))
    }
}

/// File operations for the console character device.
pub struct ConsoleFileOps;

impl FileOps for ConsoleFileOps {
    fn read(&self, _offset: usize, _buf: &mut [u8]) -> Result<usize, VfsError> {
        // Console device: no input available (returns EOF).
        Ok(0)
    }

    fn write(&self, _offset: usize, buf: &[u8]) -> Result<usize, VfsError> {
        if let Ok(s) = core::str::from_utf8(buf) {
            log::info!("[CONSOLE] {}", s.trim_end());
        }
        Ok(buf.len())
    }
}
