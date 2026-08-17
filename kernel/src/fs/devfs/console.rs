use alloc::sync::Arc;
use crate::drivers::gpu::framebuffer::fb_console_write_byte;
use crate::fs::vfs::types::{FileOps, InodeOps, VfsError};

pub fn try_read_console_byte() -> Option<u8> {
    // 1. Drain pending scancodes directly from PS/2 controller (port 0x64/0x60)
    // SAFETY: Reading status port 0x64 has no side effects and reading 0x60 when output buffer is full retrieves hardware scancode.
    let status = unsafe { crate::arch::ports::Ports::inb(0x64) };
    if (status & 0x01) != 0 && (status & 0x20) == 0 {
        let scancode = unsafe { crate::arch::ports::Ports::inb(0x60) };
        crate::drivers::char::keyboard::handle_scancode(scancode);
    }

    // 2. Check PS/2 keyboard buffer
    if let Some(byte) = crate::drivers::char::keyboard::KEY_RING_BUFFER.pop() {
        return Some(byte);
    }

    // 3. Check COM1 Serial Port (0x3F8) Line Status Register (0x3FD)
    // Bit 0 of LSR (0x3FD) is Data Ready (DR)
    // SAFETY: Reading standard COM1 16550 UART I/O ports.
    let lsr = unsafe { crate::arch::ports::Ports::inb(0x3FD) };
    if (lsr & 0x01) != 0 {
        // Data is ready on COM1 data port 0x3F8
        let byte = unsafe { crate::arch::ports::Ports::inb(0x3F8) };
        let byte = if byte == b'\r' { b'\n' } else { byte };
        return Some(byte);
    }

    None
}

/// Inode for the `/dev/console` device.
pub struct ConsoleInode;

impl InodeOps for ConsoleInode {
    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        Ok(Arc::new(ConsoleFileOps))
    }
}

/// File operations for the console character device.
pub struct ConsoleFileOps;

impl FileOps for ConsoleFileOps {
    fn read(&self, _offset: usize, buf: &mut [u8]) -> Result<usize, VfsError> {
        if buf.is_empty() {
            return Ok(0);
        }

        let mut read_bytes = 0;

        // Block until at least one character is available, then drain what is immediately ready.
        while read_bytes < buf.len() {
            if let Some(ch) = try_read_console_byte() {
                buf[read_bytes] = ch;
                read_bytes += 1;
            } else if read_bytes > 0 {
                break;
            } else {
                core::hint::spin_loop();
                crate::sched::schedule(true);
            }
        }

        Ok(read_bytes)
    }

    fn write(&self, _offset: usize, buf: &[u8]) -> Result<usize, VfsError> {
        for &byte in buf {
            fb_console_write_byte(byte);
        }
        if let Ok(s) = core::str::from_utf8(buf) {
            log::trace!("[CONSOLE] {}", s.trim_end());
        }
        Ok(buf.len())
    }
}
