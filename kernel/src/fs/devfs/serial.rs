//! Serial Port Character Device (/dev/ttyS0)
//!
//! Provides the VFS interface for 16550 UART COM1 serial communication.

use alloc::sync::Arc;
use crate::device::CharDevice;
use crate::drivers::serial::{PortIoBackend, SerialPort};
use crate::fs::vfs::types::{FileOps, InodeOps, Stat, VfsError, O_NONBLOCK};
use crate::sync::Mutex;
use crate::syscalls::fs::{POLLIN, POLLOUT};

/// Global instance for /dev/ttyS0 VFS node operations.
static TTY_S0: Mutex<SerialPort<PortIoBackend>> = Mutex::new(SerialPort::new(PortIoBackend::new(0x3F8)));

/// Inode for the `/dev/ttyS0` device.
pub struct SerialInode;

impl InodeOps for SerialInode {
    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        Ok(Arc::new(SerialFileOps))
    }

    fn stat(&self) -> Result<Stat, VfsError> {
        Ok(Stat {
            mode: 0o020660, // S_IFCHR | 0660
            nlink: 1,
            ..Default::default()
        })
    }
}

/// File operations for `/dev/ttyS0`.
pub struct SerialFileOps;

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
            let port = TTY_S0.lock();
            while read_count < buf.len() {
                if let Some(b) = port.try_read_byte() {
                    buf[read_count] = b;
                    read_count += 1;
                } else {
                    break;
                }
            }
            drop(port);

            if read_count > 0 || non_blocking {
                if read_count > 0 {
                    return Ok(read_count);
                } else {
                    return Err(VfsError::WouldBlock);
                }
            }

            // Blocking read: pause until interrupt / data available
            crate::arch::enable_and_hlt();
        }
    }

    fn write(&self, _offset: usize, buf: &[u8]) -> Result<usize, VfsError> {
        let mut port = TTY_S0.lock();
        for &byte in buf {
            while !port.is_tx_ready() {
                core::hint::spin_loop();
            }
            port.write_byte(byte).map_err(|e| VfsError::DriverError(e))?;
        }
        Ok(buf.len())
    }

    fn isatty(&self) -> bool {
        true
    }

    fn poll_events(&self, events: i16) -> i16 {
        let mut revents = 0;
        let port = TTY_S0.lock();
        if (events & POLLOUT) != 0 && port.is_tx_ready() {
            revents |= POLLOUT;
        }
        if (events & POLLIN) != 0 && port.is_rx_ready() {
            revents |= POLLIN;
        }
        revents
    }
}
