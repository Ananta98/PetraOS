//! Serial Port Character Device (/dev/ttyS0)
//!
//! Provides the VFS interface for 16550 UART COM1 serial communication.

use crate::device::Device;
use crate::fs::vfs::types::{FileOps, InodeOps, O_NONBLOCK, Stat, VfsError};
use crate::sync::Mutex;
use crate::syscalls::fs::{POLLIN, POLLOUT};
use alloc::boxed::Box;
use alloc::sync::Arc;

/// Inode for the `/dev/ttyS0` device.
pub struct SerialInode {
    pub device: Arc<Mutex<Box<dyn Device>>>,
}

impl InodeOps for SerialInode {
    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        Ok(Arc::new(SerialFileOps {
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

/// File operations for `/dev/ttyS0`.
pub struct SerialFileOps {
    device: Arc<Mutex<Box<dyn Device>>>,
}

impl FileOps for SerialFileOps {
    fn read(&self, _offset: usize, buf: &mut [u8]) -> Result<usize, VfsError> {
        self.read_with_flags(_offset, buf, 0)
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

        let non_blocking = (flags & O_NONBLOCK) != 0;
        let mut read_count = 0;

        loop {
            let mut dev = self.device.lock();
            if let Some(char_dev) = dev.as_char_device_mut() {
                while read_count < buf.len() {
                    match char_dev.read_byte() {
                        Ok(b) => {
                            buf[read_count] = b;
                            read_count += 1;
                        }
                        Err(_) => break,
                    }
                }
            } else {
                return Err(VfsError::NotSupported);
            }
            drop(dev);

            if read_count > 0 || non_blocking {
                if read_count > 0 {
                    return Ok(read_count);
                } else {
                    return Err(VfsError::WouldBlock);
                }
            }

            // Yield to scheduler while waiting for serial RX
            crate::sched::schedule(true);
        }
    }

    fn write(&self, _offset: usize, buf: &[u8]) -> Result<usize, VfsError> {
        let mut dev = self.device.lock();
        let char_dev = dev.as_char_device_mut().ok_or(VfsError::NotSupported)?;
        for &byte in buf {
            char_dev.write_byte(byte).map_err(VfsError::DriverError)?;
        }
        Ok(buf.len())
    }

    fn isatty(&self) -> bool {
        true
    }

    fn poll_events(&self, events: i16) -> i16 {
        let mut revents = 0;
        if (events & POLLOUT) != 0 {
            revents |= POLLOUT;
        }
        if (events & POLLIN) != 0 {
            let dev = self.device.lock();
            if let Some(char_dev) = dev.as_char_device() {
                if char_dev.has_input() {
                    revents |= POLLIN;
                }
            }
        }
        revents
    }
}
