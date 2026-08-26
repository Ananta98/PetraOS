//! Console Character Device (/dev/console, /dev/tty, /dev/tty0)
//!
//! Provides the VFS interface for the primary console character device,
//! routing operations directly to the kernel TTY subsystem and line discipline.

use crate::fs::vfs::types::{FileOps, InodeOps, Stat, VfsError};
use crate::mm::UserPtr;
use crate::tty::console::CONSOLE;
use crate::tty::termios::{
    FIONREAD, KDGETMODE, KDSETMODE, KDGKBMODE, KDSKBMODE, KD_TEXT, K_UNICODE,
    TCFLSH, TCGETA, TCGETS, TCGETS2, TCSBRK, TCSETA, TCSETAF, TCSETAW, TCSETS,
    TCSETS2, TCSETSF, TCSETSF2, TCSETSW, TCSETSW2, TCXONC, TIOCGPGRP, TIOCGSID,
    TIOCGWINSZ, TIOCLINUX, TIOCNOTTY, TIOCOUTQ, TIOCSCTTY, TIOCSPGRP, TIOCSTI,
    TIOCSWINSZ, VT_ACTIVATE, VT_GETMODE, VT_GETSTATE, VT_OPENQRY, VT_SETMODE,
    VT_WAITACTIVE, Termio, Termios, Termios2, WinSize,
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

        log::debug!("[devfs::console ioctl] cmd={:#x} arg={:#x}", cmd, arg);
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
            TCGETS2 => {
                let ptr = UserPtr::<Termios2>::from_u64(arg as u64);
                let t2 = Termios2::from(console.ldisc.termios);
                ptr.write(t2).ok_or(VfsError::InvalidInput)?;
                Ok(0)
            }
            TCSETS2 | TCSETSW2 | TCSETSF2 => {
                let ptr = UserPtr::<Termios2>::from_u64(arg as u64);
                let t2 = ptr.read().ok_or(VfsError::InvalidInput)?;
                console.ldisc.termios = Termios::from(t2);
                Ok(0)
            }
            TCGETA => {
                let ptr = UserPtr::<Termio>::from_u64(arg as u64);
                let t = Termio::from(console.ldisc.termios);
                ptr.write(t).ok_or(VfsError::InvalidInput)?;
                Ok(0)
            }
            TCSETA | TCSETAW | TCSETAF => {
                let ptr = UserPtr::<Termio>::from_u64(arg as u64);
                let t = ptr.read().ok_or(VfsError::InvalidInput)?;
                console.ldisc.termios.c_iflag = t.c_iflag as u32;
                console.ldisc.termios.c_oflag = t.c_oflag as u32;
                console.ldisc.termios.c_cflag = t.c_cflag as u32;
                console.ldisc.termios.c_lflag = t.c_lflag as u32;
                console.ldisc.termios.c_line = t.c_line;
                console.ldisc.termios.c_cc[..8].copy_from_slice(&t.c_cc);
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
            TIOCGSID => {
                let ptr = UserPtr::<i32>::from_u64(arg as u64);
                let sid = console.ldisc.foreground_pgid;
                ptr.write(sid).ok_or(VfsError::InvalidInput)?;
                Ok(0)
            }
            TIOCOUTQ => {
                let ptr = UserPtr::<i32>::from_u64(arg as u64);
                ptr.write(0).ok_or(VfsError::InvalidInput)?;
                Ok(0)
            }
            TIOCSTI => {
                let ptr = UserPtr::<u8>::from_u64(arg as u64);
                let byte = ptr.read().ok_or(VfsError::InvalidInput)?;
                let _ = console.ldisc.accept_input_byte(byte);
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
            KDGETMODE => {
                let ptr = UserPtr::<i32>::from_u64(arg as u64);
                ptr.write(KD_TEXT as i32).ok_or(VfsError::InvalidInput)?;
                Ok(0)
            }
            KDSETMODE => Ok(0),
            KDGKBMODE => {
                let ptr = UserPtr::<i32>::from_u64(arg as u64);
                ptr.write(K_UNICODE as i32).ok_or(VfsError::InvalidInput)?;
                Ok(0)
            }
            KDSKBMODE => Ok(0),
            VT_OPENQRY => {
                let ptr = UserPtr::<i32>::from_u64(arg as u64);
                ptr.write(1).ok_or(VfsError::InvalidInput)?;
                Ok(0)
            }
            VT_GETMODE | VT_SETMODE | VT_GETSTATE | VT_ACTIVATE | VT_WAITACTIVE => Ok(0),
            TIOCLINUX => Ok(0),
            _ => Err(VfsError::NotSupported),
        }
    }
}
