//! Console Character Device (/dev/console, /dev/tty, /dev/tty0)
//!
//! Provides the VFS interface for the primary console character device,
//! routing operations directly to the kernel TTY subsystem and line discipline.

use crate::fs::vfs::types::{FileOps, InodeOps, Stat, VfsError};
use crate::mm::UserPtr;
use crate::tty::console::CONSOLE;
use crate::tty::termios::{
    FIONREAD, TCFLSH, TCGETS, TCSBRK, TCSETS, TCSETSF, TCSETSW, TCXONC, TIOCGPGRP, TIOCGWINSZ,
    TIOCNOTTY, TIOCSCTTY, TIOCSPGRP, TIOCSWINSZ, Termios, WinSize,
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
        tty_read(buf, false)
    }

    fn read_with_flags(
        &self,
        _offset: usize,
        buf: &mut [u8],
        flags: u32,
    ) -> Result<usize, VfsError> {
        let non_blocking = (flags & crate::fs::vfs::types::O_NONBLOCK) != 0;
        tty_read(buf, non_blocking)
    }

    fn write(&self, _offset: usize, buf: &[u8]) -> Result<usize, VfsError> {
        tty_write(buf);
        Ok(buf.len())
    }

    fn isatty(&self) -> bool {
        true
    }

    fn poll_events(&self, events: i16) -> i16 {
        let mut revents = 0;
        if (events & crate::syscalls::fs::POLLOUT) != 0 {
            revents |= crate::syscalls::fs::POLLOUT;
        }
        if (events & crate::syscalls::fs::POLLIN) != 0 {
            let mut guard = CONSOLE.lock();
            if let Some(ref mut c) = *guard {
                c.poll_input();
                if c.available_input() > 0 {
                    revents |= crate::syscalls::fs::POLLIN;
                }
            }
        }
        revents
    }

    fn ioctl(&self, cmd: u64, arg: usize) -> Result<usize, VfsError> {
        let mut guard = CONSOLE.lock();
        let console = guard.as_mut().ok_or(VfsError::NotFound)?;

        match cmd {
            TCGETS => {
                let ptr = UserPtr::<Termios>::from_u64(arg as u64);
                let t = console.ldisc.termios;
                ptr.write(t).ok_or(VfsError::InvalidInput)?;
                Ok(0)
            }
            TCSETS | TCSETSW | TCSETSF => {
                let ptr = UserPtr::<Termios>::from_u64(arg as u64);
                let t = ptr.read().ok_or(VfsError::InvalidInput)?;
                console.ldisc.termios = t;
                Ok(0)
            }
            TIOCGWINSZ => {
                let ptr = UserPtr::<WinSize>::from_u64(arg as u64);
                let ws = console.ldisc.winsize;
                ptr.write(ws).ok_or(VfsError::InvalidInput)?;
                Ok(0)
            }
            TIOCSWINSZ => {
                let ptr = UserPtr::<WinSize>::from_u64(arg as u64);
                let ws = ptr.read().ok_or(VfsError::InvalidInput)?;
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
                let ptr = UserPtr::<i32>::from_u64(arg as u64);
                let pgid = console.ldisc.foreground_pgid;
                ptr.write(pgid).ok_or(VfsError::InvalidInput)?;
                Ok(0)
            }
            TIOCSPGRP => {
                let ptr = UserPtr::<i32>::from_u64(arg as u64);
                let pgid = ptr.read().ok_or(VfsError::InvalidInput)?;
                console.ldisc.foreground_pgid = pgid;
                Ok(0)
            }
            TIOCSCTTY => Ok(0),
            TIOCNOTTY => Ok(0),
            TCSBRK => Ok(0),
            TCXONC => Ok(0),
            TCFLSH => {
                console.ldisc.flush_queue(arg);
                Ok(0)
            }
            FIONREAD => {
                let ptr = UserPtr::<i32>::from_u64(arg as u64);
                let len = console.available_input() as i32;
                ptr.write(len).ok_or(VfsError::InvalidInput)?;
                Ok(0)
            }
            _ => Err(VfsError::NotSupported),
        }
    }
}
