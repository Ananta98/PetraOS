// SPDX-License-Identifier: GPL-2.0

//! TTY Line Discipline
//!
//! Provides buffering, canonical editing, and signal generation.

use alloc::collections::VecDeque;
use spin::Mutex;

use crate::api::ioctl;
use crate::api::termios::{
    winsize, Termios, ECHO, ECHOK, ICANON, ICRNL, IGNCR, INLCR, ISIG, VEOF, VEOL, VERASE, VINTR,
    VKILL, VQUIT, VSUSP,
};
use crate::api::errno::{Errno, Result};
use crate::proc::thread::Thread;
use crate::utils::userspace::user_ptr::UserPtr;

/// Internal state of the line discipline
#[derive(Debug, Default)]
pub struct LdiscState {
    pub eof_received: bool,
    pub esc_pending: bool,
}

/// A TTY Line Discipline instance
pub struct LineDiscipline {
    pub termios: Mutex<Termios>,
    pub winsize: Mutex<winsize>,
    pub read_buf: Mutex<VecDeque<u8>>,
    pub canon_buf: Mutex<VecDeque<u8>>,
    pub state: Mutex<LdiscState>,
    pub pgrp: Mutex<Option<crate::proc::tree::Pid>>,
}

impl LineDiscipline {
    pub fn new() -> Self {
        Self {
            termios: Mutex::new(Termios::default()),
            winsize: Mutex::new(winsize {
                ws_row: 50,
                ws_col: 160,
                ws_xpixel: 1280,
                ws_ypixel: 800,
            }),
            read_buf: Mutex::new(VecDeque::with_capacity(4096)),
            canon_buf: Mutex::new(VecDeque::with_capacity(4096)),
            state: Mutex::new(LdiscState::default()),
            pgrp: Mutex::new(None),
        }
    }

    /// Read data from the line discipline buffer
    pub fn read(&self, buf: &mut [u8]) -> Result<usize> {
        let termios = self.termios.lock();
        let icanon = (termios.c_lflag & ICANON) != 0;
        drop(termios);

        if icanon {
            self.read_canon(buf)
        } else {
            self.read_raw(buf)
        }
    }

    fn read_canon(&self, buf: &mut [u8]) -> Result<usize> {
        let mut canon_buf = self.canon_buf.lock();
        if canon_buf.is_empty() {
            return Ok(0);
        }

        let mut read = 0;
        for b in buf.iter_mut() {
            if let Some(c) = canon_buf.pop_front() {
                *b = c;
                read += 1;
                if c == b'\n' {
                    break;
                }
            } else {
                break;
            }
        }
        Ok(read)
    }

    fn read_raw(&self, buf: &mut [u8]) -> Result<usize> {
        let mut read_buf = self.read_buf.lock();
        if read_buf.is_empty() {
            return Ok(0);
        }

        let mut read = 0;
        for b in buf.iter_mut() {
            if let Some(c) = read_buf.pop_front() {
                *b = c;
                read += 1;
            } else {
                break;
            }
        }
        Ok(read)
    }

    /// Receive a character from the hardware driver
    pub fn receive_char(&self, mut ch: u8, echo_out: &mut impl FnMut(u8)) {
        let termios = *self.termios.lock();
        let lflag = termios.c_lflag;
        let iflag = termios.c_iflag;
        let cc = termios.c_cc;

        // Input processing (ICRNL, IGNCR, INLCR)
        if ch == b'\r' {
            if (iflag & IGNCR) != 0 {
                return;
            }
            if (iflag & ICRNL) != 0 {
                ch = b'\n';
            }
        } else if ch == b'\n' {
            if (iflag & INLCR) != 0 {
                ch = b'\r';
            }
        }

        let isig = (lflag & ISIG) != 0;
        let icanon = (lflag & ICANON) != 0;
        let echo = (lflag & ECHO) != 0;

        // Signal generation
        if isig {
            let target_pgrp = *self.pgrp.lock();
            let signal_group = |sig: crate::api::signal::Signal| {
                let pgrp_opt = target_pgrp.or_else(|| Thread::current_proc().map(|p| p.pgid()));
                if let Some(pgrp) = pgrp_opt {
                    for proc in crate::proc::tree::allproc_iter().filter(|p| p.pgid() == pgrp) {
                        proc.post_signal(sig);
                    }
                }
            };

            if ch == cc[VINTR] {
                signal_group(crate::api::signal::Signal::SIGINT);
                if echo {
                    echo_out(b'^');
                    echo_out(b'C');
                }
                return;
            }
            if ch == cc[VQUIT] {
                signal_group(crate::api::signal::Signal::SIGQUIT);
                if echo {
                    echo_out(b'^');
                    echo_out(b'\\');
                }
                return;
            }
            if ch == cc[VSUSP] {
                signal_group(crate::api::signal::Signal::SIGTSTP);
                if echo {
                    echo_out(b'^');
                    echo_out(b'Z');
                }
                return;
            }
        }

        if icanon {
            let mut read_buf = self.read_buf.lock();
            let mut canon_buf = self.canon_buf.lock();

            if ch == cc[VEOF] {
                while let Some(b) = read_buf.pop_front() {
                    canon_buf.push_back(b);
                }
                return;
            }

            if ch == cc[VERASE] || ch == 8 || ch == 127 {
                // Backspace or DEL
                if read_buf.pop_back().is_some() && echo {
                    echo_out(8);
                    echo_out(b' ');
                    echo_out(8);
                }
                return;
            }

            if ch == cc[VKILL] {
                while read_buf.pop_back().is_some() {
                    if echo && (lflag & ECHOK) != 0 {
                        echo_out(8);
                        echo_out(b' ');
                        echo_out(8);
                    }
                }
                return;
            }

            // Normal character
            read_buf.push_back(ch);
            if echo {
                echo_out(ch);
            }

            if ch == b'\n' || ch == cc[VEOL] {
                // Line complete
                while let Some(b) = read_buf.pop_front() {
                    canon_buf.push_back(b);
                }
            }
        } else {
            // Raw mode
            let mut read_buf = self.read_buf.lock();
            read_buf.push_back(ch);
            if echo {
                echo_out(ch);
            }
        }
    }

    /// Ioctl handler
    pub fn ioctl(&self, cmd: u32, arg: usize) -> Result<usize> {
        let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
        let vmar = proc.vmspace();

        match cmd {
            ioctl::TCGETS => {
                let termios = *self.termios.lock();
                let bytes = termios.to_bytes();
                UserPtr::new(arg).write_bytes(&vmar, &bytes)?;
                Ok(0)
            }
            ioctl::TCSETS | ioctl::TCSETSW | ioctl::TCSETSF => {
                let mut bytes = [0u8; Termios::SIZE];
                UserPtr::new(arg).read_bytes(&vmar, &mut bytes)?;
                let new_termios = Termios::from_bytes(&bytes);
                *self.termios.lock() = new_termios;
                if cmd == ioctl::TCSETSF {
                    self.read_buf.lock().clear();
                    self.canon_buf.lock().clear();
                }
                Ok(0)
            }
            ioctl::TCFLSH => {
                if arg == 0 || arg == 2 {
                    self.read_buf.lock().clear();
                    self.canon_buf.lock().clear();
                }
                Ok(0)
            }
            ioctl::TIOCGWINSZ => {
                let winsize = *self.winsize.lock();
                let bytes = winsize.to_bytes();
                UserPtr::new(arg).write_bytes(&vmar, &bytes)?;
                Ok(0)
            }
            ioctl::TIOCSWINSZ => {
                let mut bytes = [0u8; winsize::SIZE];
                UserPtr::new(arg).read_bytes(&vmar, &mut bytes)?;
                let new_winsize = winsize::from_bytes(&bytes);
                *self.winsize.lock() = new_winsize;
                Ok(0)
            }
            ioctl::FIONREAD => {
                let count = if (self.termios.lock().c_lflag & ICANON) != 0 {
                    self.canon_buf.lock().len()
                } else {
                    self.read_buf.lock().len()
                } as i32;
                UserPtr::new(arg).write_bytes(&vmar, &count.to_ne_bytes())?;
                Ok(0)
            }
            ioctl::TIOCSPGRP => {
                let mut pid_bytes = [0u8; 4];
                UserPtr::new(arg).read_bytes(&vmar, &mut pid_bytes)?;
                let raw_pid = i32::from_ne_bytes(pid_bytes);
                if raw_pid < 0 {
                    crate::return_errno!(EINVAL, "invalid process group ID");
                }
                let pid = crate::proc::tree::Pid::from_u32(raw_pid as u32)
                    .ok_or(Errno::EINVAL)?;
                *self.pgrp.lock() = Some(pid);
                Ok(0)
            }
            ioctl::TIOCGPGRP => {
                let pgrp = self.pgrp.lock().map(|p| p.as_u32() as i32).unwrap_or_else(|| {
                    proc.pgid().as_u32() as i32
                });
                UserPtr::new(arg).write_bytes(&vmar, &pgrp.to_ne_bytes())?;
                Ok(0)
            }
            ioctl::TIOCSCTTY => {
                *self.pgrp.lock() = Some(proc.pgid());
                Ok(0)
            }
            ioctl::TIOCNOTTY => {
                *self.pgrp.lock() = None;
                Ok(0)
            }
            _ => crate::return_errno!(ENOTTY, "inappropriate ioctl for device"),
        }
    }
}
