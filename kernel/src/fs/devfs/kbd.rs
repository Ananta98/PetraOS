//! Keyboard Character Device (/dev/kbd, /dev/input/event0)
//!
//! Provides raw character and keystroke reading from the PS/2 keyboard ring buffer.

use alloc::sync::Arc;
use crate::drivers::char::keyboard::KEY_RING_BUFFER;
use crate::fs::vfs::types::{FileOps, InodeOps, Stat, VfsError, O_NONBLOCK};
use crate::syscalls::fs::POLLIN;

/// Inode for the `/dev/kbd` device.
pub struct KbdInode;

impl InodeOps for KbdInode {
    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        Ok(Arc::new(KbdFileOps))
    }

    fn stat(&self) -> Result<Stat, VfsError> {
        Ok(Stat {
            mode: 0o020600, // S_IFCHR | 0600
            nlink: 1,
            ..Default::default()
        })
    }
}

/// File operations for `/dev/kbd`.
pub struct KbdFileOps;

impl FileOps for KbdFileOps {
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
            while read_count < buf.len() {
                if let Some(byte) = KEY_RING_BUFFER.pop() {
                    buf[read_count] = byte;
                    read_count += 1;
                } else {
                    break;
                }
            }

            if read_count > 0 || non_blocking {
                if read_count > 0 {
                    return Ok(read_count);
                } else {
                    return Err(VfsError::WouldBlock);
                }
            }

            // Blocking read: pause until next keyboard interrupt
            crate::arch::enable_and_hlt();
        }
    }

    fn write(&self, _offset: usize, buf: &[u8]) -> Result<usize, VfsError> {
        Ok(buf.len())
    }

    fn poll_events(&self, events: i16) -> i16 {
        let mut revents = 0;
        if (events & POLLIN) != 0 && !KEY_RING_BUFFER.is_empty() {
            revents |= POLLIN;
        }
        revents
    }
}
