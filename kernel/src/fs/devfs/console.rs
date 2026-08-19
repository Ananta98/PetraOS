//! Console Character Device (/dev/console, /dev/tty, /dev/tty0)
//!
//! Provides the VFS interface for the primary console character device,
//! routing operations directly to the kernel TTY subsystem and line discipline.

use crate::fs::vfs::types::{FileOps, InodeOps, Stat, VfsError};
use crate::tty::console::CONSOLE;
use crate::tty::termios::{
    FIONREAD, TCGETS, TCSETS, TCSETSF, TCSETSW, TIOCGPGRP, TIOCGWINSZ, TIOCNOTTY, TIOCSCTTY,
    TIOCSPGRP, TIOCSWINSZ, Termios, WinSize,
};
use crate::tty::{tty_read, tty_write};
use alloc::sync::Arc;

/// Inode for the `/dev/console` device.
pub struct ConsoleInode;

impl InodeOps for ConsoleInode {
    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        Ok(Arc::new(ConsoleFileOps))
    }

    fn stat(&self) -> Result<Stat, VfsError> {
        Ok(Stat {
            mode: 0o020666, // S_IFCHR | 0666
            nlink: 1,
            ..Default::default()
        })
    }
}

/// File operations for the console character device.
pub struct ConsoleFileOps;

impl FileOps for ConsoleFileOps {
    fn read(&self, _offset: usize, buf: &mut [u8]) -> Result<usize, VfsError> {
        tty_read(buf)
    }

    fn write(&self, _offset: usize, buf: &[u8]) -> Result<usize, VfsError> {
        tty_write(buf);
        Ok(buf.len())
    }

    fn ioctl(&self, cmd: u64, arg: usize) -> Result<usize, VfsError> {
        let mut guard = CONSOLE.lock();
        let console = guard.as_mut().ok_or(VfsError::NotFound)?;

        match cmd {
            TCGETS => {
                if !crate::syscalls::is_user_ptr_valid(arg as u64, core::mem::size_of::<Termios>())
                {
                    return Err(VfsError::InvalidInput);
                }
                let t = console.ldisc.termios;
                // SAFETY: User pointer validated within user space bounds.
                unsafe {
                    *(arg as *mut Termios) = t;
                }
                Ok(0)
            }
            TCSETS | TCSETSW | TCSETSF => {
                if !crate::syscalls::is_user_ptr_valid(arg as u64, core::mem::size_of::<Termios>())
                {
                    return Err(VfsError::InvalidInput);
                }
                // SAFETY: User pointer validated within user space bounds.
                let t = unsafe { *(arg as *const Termios) };
                console.ldisc.termios = t;
                Ok(0)
            }
            TIOCGWINSZ => {
                if !crate::syscalls::is_user_ptr_valid(arg as u64, core::mem::size_of::<WinSize>())
                {
                    return Err(VfsError::InvalidInput);
                }
                let ws = console.ldisc.winsize;
                // SAFETY: User pointer validated within user space bounds.
                unsafe {
                    *(arg as *mut WinSize) = ws;
                }
                Ok(0)
            }
            TIOCSWINSZ => {
                if !crate::syscalls::is_user_ptr_valid(arg as u64, core::mem::size_of::<WinSize>())
                {
                    return Err(VfsError::InvalidInput);
                }
                // SAFETY: User pointer validated within user space bounds.
                let ws = unsafe { *(arg as *const WinSize) };
                console.ldisc.winsize = ws;
                if console.ldisc.foreground_pgid > 0 {
                    let _ = crate::ipc::signal::send_signal_to_process_group(
                        console.ldisc.foreground_pgid,
                        crate::ipc::signal::SIGWINCH,
                    );
                }
                Ok(0)
            }
            TIOCGPGRP => {
                if !crate::syscalls::is_user_ptr_valid(arg as u64, 4) {
                    return Err(VfsError::InvalidInput);
                }
                let pgid = console.ldisc.foreground_pgid;
                // SAFETY: User pointer validated within user space bounds.
                unsafe {
                    *(arg as *mut i32) = pgid;
                }
                Ok(0)
            }
            TIOCSPGRP => {
                if !crate::syscalls::is_user_ptr_valid(arg as u64, 4) {
                    return Err(VfsError::InvalidInput);
                }
                // SAFETY: User pointer validated within user space bounds.
                let pgid = unsafe { *(arg as *const i32) };
                console.ldisc.foreground_pgid = pgid;
                Ok(0)
            }
            TIOCSCTTY => Ok(0),
            TIOCNOTTY => Ok(0),
            FIONREAD => {
                if !crate::syscalls::is_user_ptr_valid(arg as u64, 4) {
                    return Err(VfsError::InvalidInput);
                }
                let len = console.available_input() as i32;
                // SAFETY: User pointer validated within user space bounds.
                unsafe {
                    *(arg as *mut i32) = len;
                }
                Ok(0)
            }
            _ => Err(VfsError::NotSupported),
        }
    }

    fn isatty(&self) -> bool {
        true
    }
}
