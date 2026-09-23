//! Mouse Character Device (/dev/mice, /dev/input/mice, /dev/psaux)
//!
//! Bridges the PS/2 mouse packet stream from `MOUSE_RING_BUFFER` to userland
//! VFS device nodes. Supports blocking and non-blocking reads, and `poll()` / `select()`
//! event notifications.

use alloc::sync::Arc;
use crate::drivers::char::mouse::{MOUSE_RING_BUFFER, MOUSE_WAIT_QUEUE};
use crate::fs::vfs::types::{FileOps, InodeOps, Stat, VfsError, O_NONBLOCK};
use crate::syscalls::fs::{POLLIN, POLLOUT};

/// Inode for `/dev/mice`, `/dev/input/mice`, and `/dev/psaux`.
pub struct MouseInode;

impl InodeOps for MouseInode {
    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        Ok(Arc::new(MouseFileOps))
    }

    fn stat(&self) -> Result<Stat, VfsError> {
        Ok(Stat {
            mode: 0o020660, // S_IFCHR | 0660
            nlink: 1,
            rdev: (13 << 8) | 63, // Linux major 13 (input), minor 63 (mice)
            ..Default::default()
        })
    }
}

/// File operations for mouse character device nodes.
pub struct MouseFileOps;

impl FileOps for MouseFileOps {
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

        let non_blocking = (flags & O_NONBLOCK) != 0;

        loop {
            let mut read_count = 0;
            while read_count < buf.len() {
                if let Some(byte) = MOUSE_RING_BUFFER.pop() {
                    buf[read_count] = byte;
                    read_count += 1;
                } else {
                    break;
                }
            }

            if read_count > 0 {
                return Ok(read_count);
            }

            if non_blocking {
                return Err(VfsError::WouldBlock);
            }

            // Blocking read: wait until next mouse event
            MOUSE_WAIT_QUEUE.wait();
        }
    }

    fn write(&self, _offset: usize, buf: &[u8]) -> Result<usize, VfsError> {
        // Writing to mouse device node can accept commands in standard UNIX
        Ok(buf.len())
    }

    fn stat(&self) -> Result<Stat, VfsError> {
        Ok(Stat {
            mode: 0o020660, // S_IFCHR | 0660
            nlink: 1,
            rdev: (13 << 8) | 63,
            ..Default::default()
        })
    }

    fn poll_events(&self, events: i16) -> i16 {
        let mut revents = 0;
        if (events & POLLIN) != 0 && !MOUSE_RING_BUFFER.is_empty() {
            revents |= POLLIN;
        }
        if (events & POLLOUT) != 0 {
            revents |= POLLOUT;
        }
        revents
    }
}
