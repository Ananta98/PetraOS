//! Generic Character Device VFS Inode and FileOps
//!
//! Bridges any character device registered in `DEVICE_MANAGER` to a `/dev` character node.

use alloc::boxed::Box;
use alloc::sync::Arc;
use crate::device::{Device, DEVICE_MANAGER};
use crate::fs::vfs::types::{FileOps, InodeOps, Stat, VfsError};
use crate::sync::Mutex;

/// Inode for dynamically registered character devices in devfs.
pub struct GenericCharDeviceInode {
    pub device_name: &'static str,
}

impl InodeOps for GenericCharDeviceInode {
    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        let device = DEVICE_MANAGER
            .read()
            .get_by_name(self.device_name)
            .ok_or(VfsError::NotFound)?;

        Ok(Arc::new(GenericCharDeviceFileOps { device }))
    }

    fn stat(&self) -> Result<Stat, VfsError> {
        Ok(Stat {
            mode: 0o020660, // S_IFCHR | 0660
            nlink: 1,
            ..Default::default()
        })
    }
}

/// Per-open file operations for a generic character device node.
pub struct GenericCharDeviceFileOps {
    device: Arc<Mutex<Box<dyn Device>>>,
}

impl FileOps for GenericCharDeviceFileOps {
    fn read(&self, _offset: usize, buf: &mut [u8]) -> Result<usize, VfsError> {
        if buf.is_empty() {
            return Ok(0);
        }

        let mut dev_lock = self.device.lock();
        let char_dev = dev_lock.as_char_device_mut().ok_or(VfsError::NotSupported)?;

        let mut read_bytes = 0;
        for slot in buf.iter_mut() {
            match char_dev.read_byte() {
                Ok(b) => {
                    *slot = b;
                    read_bytes += 1;
                }
                Err(_) => break,
            }
        }

        if read_bytes > 0 {
            Ok(read_bytes)
        } else {
            Err(VfsError::WouldBlock)
        }
    }

    fn write(&self, _offset: usize, buf: &[u8]) -> Result<usize, VfsError> {
        let mut dev_lock = self.device.lock();
        let char_dev = dev_lock.as_char_device_mut().ok_or(VfsError::NotSupported)?;

        for &b in buf {
            char_dev.write_byte(b).map_err(|e| VfsError::DriverError(e))?;
        }
        Ok(buf.len())
    }
}
