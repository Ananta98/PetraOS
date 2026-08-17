//! TTY and Terminal Subsystem
//!
//! Implements POSIX terminal line discipline, termios management,
//! high-performance framebuffer text console rendering, and terminal IOCTL handling.

pub mod fb_console;
pub mod font;
pub mod termios;

pub use fb_console::{
    fb_console_available, fb_console_init, fb_console_write_byte, fb_console_write_str,
    fb_get_console_size,
};
pub use termios::*;

use crate::fs::vfs::types::{InodeType, VfsError};

/// Write a single byte to the active framebuffer terminal.
#[inline(always)]
pub fn tty_write_byte(byte: u8) {
    fb_console_write_byte(byte);
}

/// Write a buffer of bytes to the active terminal.
pub fn tty_write(buf: &[u8]) {
    for &byte in buf {
        tty_write_byte(byte);
    }
}

/// Check if a file descriptor refers to a terminal / character device.
pub fn isatty(fd: i32) -> Result<bool, VfsError> {
    if fd < 0 {
        return Err(VfsError::BadFd);
    }

    let proc_arc = crate::proc::current_process().ok_or(VfsError::NotFound)?;
    let proc = proc_arc.lock();
    let file = proc.fd_table.get(fd)?;
    drop(proc);

    let is_tty =
        fd == 0 || fd == 1 || fd == 2 || file.dentry.inode.inode_type == InodeType::CharDevice;

    Ok(is_tty)
}

/// Dispatch IOCTL commands on a file descriptor.
pub fn do_ioctl(fd: i32, cmd: u64, arg: usize) -> Result<usize, VfsError> {
    if fd < 0 {
        return Err(VfsError::BadFd);
    }

    let proc_arc = crate::proc::current_process().ok_or(VfsError::NotFound)?;
    let proc = proc_arc.lock();
    let file = proc.fd_table.get(fd)?;
    drop(proc);

    let is_char_dev =
        fd == 0 || fd == 1 || fd == 2 || file.dentry.inode.inode_type == InodeType::CharDevice;

    let arg_ptr = arg as *mut u8;

    match cmd {
        // --- Terminal IOCTLs ---
        TCGETS => {
            if !is_char_dev {
                return Err(VfsError::NotSupported);
            }
            if !arg_ptr.is_null() {
                if !crate::syscalls::is_user_ptr_valid(
                    arg_ptr as u64,
                    core::mem::size_of::<Termios>(),
                ) {
                    return Err(VfsError::InvalidInput);
                }
                let term = *CONSOLE_TERMIOS.lock();
                // SAFETY: User pointer validated with is_user_ptr_valid.
                unsafe {
                    core::ptr::write_volatile(arg_ptr as *mut Termios, term);
                }
            }
            Ok(0)
        }
        TIOCGWINSZ => {
            if !is_char_dev {
                return Err(VfsError::NotSupported);
            }
            if !arg_ptr.is_null() {
                if !crate::syscalls::is_user_ptr_valid(
                    arg_ptr as u64,
                    core::mem::size_of::<WinSize>(),
                ) {
                    return Err(VfsError::InvalidInput);
                }

                let (rows, cols) = fb_get_console_size().unwrap_or((25, 80));
                let ws = WinSize {
                    ws_row: rows as u16,
                    ws_col: cols as u16,
                    ws_xpixel: 0,
                    ws_ypixel: 0,
                };
                // SAFETY: User pointer validated with is_user_ptr_valid.
                unsafe {
                    core::ptr::write_volatile(arg_ptr as *mut WinSize, ws);
                }
            }
            Ok(0)
        }
        TCSETS | TCSETSW | TCSETSF => {
            if !arg_ptr.is_null() {
                if !crate::syscalls::is_user_ptr_valid(
                    arg_ptr as u64,
                    core::mem::size_of::<Termios>(),
                ) {
                    return Err(VfsError::InvalidInput);
                }
                // SAFETY: User pointer validated with is_user_ptr_valid.
                let new_term = unsafe { core::ptr::read_volatile(arg_ptr as *const Termios) };
                *CONSOLE_TERMIOS.lock() = new_term;
            }
            Ok(0)
        }
        TCGETA | TCSETA | TCSETAW | TCSETAF | TIOCSCTTY | TIOCSWINSZ | TIOCSPGRP => Ok(0),
        TIOCGPGRP => {
            if !arg_ptr.is_null() {
                if !crate::syscalls::is_user_ptr_valid(arg_ptr as u64, core::mem::size_of::<i32>())
                {
                    return Err(VfsError::InvalidInput);
                }
                let pgid = proc_arc.lock().pgid.as_u64() as i32;
                // SAFETY: User pointer validated with is_user_ptr_valid.
                unsafe {
                    core::ptr::write_volatile(arg_ptr as *mut i32, pgid);
                }
            }
            Ok(0)
        }

        // --- Framebuffer IOCTLs ---
        crate::drivers::gpu::framebuffer::FBIOGET_VSCREENINFO
        | crate::drivers::gpu::framebuffer::FBIOPUT_VSCREENINFO
        | crate::drivers::gpu::framebuffer::FBIOGET_FSCREENINFO
        | crate::drivers::gpu::framebuffer::FBIOPAN_DISPLAY
        | crate::drivers::gpu::framebuffer::FBIOBLANK => {
            crate::drivers::gpu::framebuffer::fb_ioctl(cmd, arg)
        }

        _ => Ok(0),
    }
}

/// Initializes the TTY subsystem and Framebuffer console.
pub fn init() {
    if let Err(e) = crate::drivers::gpu::framebuffer::init() {
        log::warn!("[TTY] Framebuffer driver initialization error: {:?}", e);
    } else {
        fb_console_init();
    }
}
