//! Generic Character Device VFS Inode and FileOps
//!
//! Bridges any character device registered in `DEVICE_MANAGER` to a `/dev` character node.

use alloc::boxed::Box;
use alloc::sync::Arc;
use crate::device::Device;
use crate::fs::vfs::types::{FileOps, InodeOps, Stat, VfsError};
use crate::sync::Mutex;

/// Inode for dynamically registered character devices in devfs.
pub struct GenericCharDeviceInode {
    pub device: Arc<Mutex<Box<dyn Device>>>,
}

impl InodeOps for GenericCharDeviceInode {
    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        Ok(Arc::new(GenericCharDeviceFileOps {
            device: self.device.clone(),
        }))
    }

    fn stat(&self) -> Result<Stat, VfsError> {
        let rdev = self.device.lock().rdev();
        Ok(Stat {
            mode: 0o020660, // S_IFCHR | 0660
            nlink: 1,
            rdev,
            ..Default::default()
        })
    }
}

/// Per-open file operations for a generic character device node.
pub struct GenericCharDeviceFileOps {
    device: Arc<Mutex<Box<dyn Device>>>,
}

impl FileOps for GenericCharDeviceFileOps {
    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, VfsError> {
        self.read_with_flags(offset, buf, 0)
    }

    fn read_with_flags(
        &self,
        _offset: usize,
        buf: &mut [u8],
        flags: u32,
    ) -> Result<usize, VfsError> {
        if buf.is_empty() {
            return Ok(0);
        }

        loop {
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
                return Ok(read_bytes);
            }

            // Buffer is empty
            if (flags & crate::fs::vfs::types::O_NONBLOCK) != 0 {
                return Err(VfsError::WouldBlock);
            }

            if let Some(wq) = char_dev.wait_queue() {
                drop(dev_lock);
                wq.wait();
            } else {
                drop(dev_lock);
                return Err(VfsError::WouldBlock);
            }
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
