// SPDX-License-Identifier: GPL-2.0

//! TTY ioctl Handlers.
//!
//! Implements standard POSIX/Linux termios, window sizing, and job control
//! ioctl commands for TTY devices.

use alloc::sync::Arc;
use core::sync::atomic::Ordering;

use crate::uapi::errno::{Errno, Result};
use crate::uapi::ioctl;
use crate::uapi::signal::Signal;
use crate::uapi::termios::{Termios, winsize};
use crate::proc::{PID_INIT, Pid};
use crate::utils::userspace::user_ptr::UserPtr;
use crate::vm::vmar::Vmar;

use super::input::{drain_input, flush_input_queue};
use super::tty::Tty;

pub fn handle_termios_ioctl(tty: &Tty, cmd: u32, arg: usize, vmar: &Arc<Vmar>) -> Result<usize> {
    match cmd {
        ioctl::TCGETS => {
            let termios = tty.ldisc.lock().termios;
            let bytes = termios.to_bytes();
            UserPtr::new(arg).write_bytes(vmar, &bytes)?;
            Ok(0)
        }
        ioctl::TCSETS | ioctl::TCSETSW | ioctl::TCSETSF => {
            let mut bytes = [0u8; Termios::SIZE];
            UserPtr::new(arg).read_bytes(vmar, &mut bytes)?;
            let new_termios = Termios::from_bytes(&bytes);
            let mut ldisc = tty.ldisc.lock();
            if cmd == ioctl::TCSETSF {
                flush_input_queue();
                ldisc.read_buf.clear();
                ldisc.canon_buf.clear();
            }
            ldisc.termios = new_termios;
            Ok(0)
        }
        ioctl::TCFLSH => {
            let queue = arg as u32;
            let mut ldisc = tty.ldisc.lock();
            match queue {
                0 => {
                    flush_input_queue();
                    ldisc.read_buf.clear();
                    ldisc.canon_buf.clear();
                }
                1 => {}
                2 => {
                    flush_input_queue();
                    ldisc.read_buf.clear();
                    ldisc.canon_buf.clear();
                }
                _ => return Err(Errno::EINVAL),
            }
            Ok(0)
        }
        ioctl::TCSBRK | ioctl::TCSBRKP | ioctl::TIOCSBRK | ioctl::TIOCCBRK => Ok(0),
        ioctl::TCXONC => Ok(0),
        _ => crate::return_errno!(ENOTTY, "unsupported termios ioctl"),
    }
}

pub fn handle_winsize_ioctl(tty: &Tty, cmd: u32, arg: usize, vmar: &Arc<Vmar>) -> Result<usize> {
    match cmd {
        ioctl::TIOCGWINSZ => {
            let dws = tty.driver.get_winsize();
            let mut ws = *tty.winsize.lock();
            if dws.ws_col > 0 && dws.ws_row > 0 {
                if !tty.winsize_user_set.load(Ordering::Acquire)
                    || ws.ws_col == 0
                    || ws.ws_row == 0
                {
                    ws = dws;
                    *tty.winsize.lock() = ws;
                }
            }
            let bytes = ws.to_bytes();
            UserPtr::new(arg).write_bytes(vmar, &bytes)?;
            Ok(0)
        }
        ioctl::TIOCSWINSZ => {
            let mut bytes = [0u8; winsize::SIZE];
            UserPtr::new(arg).read_bytes(vmar, &mut bytes)?;
            let ws = winsize::from_bytes(&bytes);
            *tty.winsize.lock() = ws;
            tty.winsize_user_set.store(true, Ordering::Release);
            let _ = tty.driver.set_winsize(ws);
            tty.signal_foreground(Signal::SIGWINCH);
            Ok(0)
        }
        _ => crate::return_errno!(ENOTTY, "unsupported winsize ioctl"),
    }
}

pub fn handle_job_control_ioctl(
    tty: &Tty,
    cmd: u32,
    arg: usize,
    vmar: &Arc<Vmar>,
    proc_opt: Option<&Arc<crate::proc::Proc>>,
) -> Result<usize> {
    match cmd {
        ioctl::TIOCSPGRP => {
            let mut bytes = [0u8; 4];
            UserPtr::new(arg).read_bytes(vmar, &mut bytes)?;
            let raw = u32::from_ne_bytes(bytes);
            let pgrp = Pid::from_u32(raw).ok_or(Errno::EINVAL)?;
            *tty.foreground_pgrp.lock() = Some(pgrp);
            Ok(0)
        }
        ioctl::TIOCGPGRP => {
            let pgrp = tty
                .foreground_pgrp
                .lock()
                .or_else(|| proc_opt.map(|p| p.pgid()))
                .unwrap_or(PID_INIT);
            let bytes = pgrp.as_u32().to_ne_bytes();
            UserPtr::new(arg).write_bytes(vmar, &bytes)?;
            Ok(0)
        }
        ioctl::TIOCSCTTY => {
            let proc = proc_opt.ok_or(Errno::EPERM)?;
            *tty.session.lock() = Some(proc.pid);
            *tty.foreground_pgrp.lock() = Some(proc.pgid());
            Ok(0)
        }
        ioctl::TIOCNOTTY => {
            *tty.session.lock() = None;
            *tty.foreground_pgrp.lock() = None;
            Ok(0)
        }
        ioctl::TIOCGSID => {
            let sid = tty
                .session
                .lock()
                .or_else(|| proc_opt.map(|p| p.pid))
                .unwrap_or(PID_INIT);
            let bytes = sid.as_u32().to_ne_bytes();
            UserPtr::new(arg).write_bytes(vmar, &bytes)?;
            Ok(0)
        }
        _ => crate::return_errno!(ENOTTY, "unsupported job control ioctl"),
    }
}

pub fn handle_ioctl(
    tty: &Tty,
    cmd: u32,
    arg: usize,
    vmar: &Arc<Vmar>,
    proc_opt: Option<&Arc<crate::proc::Proc>>,
) -> Result<usize> {
    match cmd {
        ioctl::TCGETS
        | ioctl::TCSETS
        | ioctl::TCSETSW
        | ioctl::TCSETSF
        | ioctl::TCFLSH
        | ioctl::TCSBRK
        | ioctl::TCSBRKP
        | ioctl::TIOCSBRK
        | ioctl::TIOCCBRK
        | ioctl::TCXONC => handle_termios_ioctl(tty, cmd, arg, vmar),

        ioctl::TIOCGWINSZ | ioctl::TIOCSWINSZ => handle_winsize_ioctl(tty, cmd, arg, vmar),

        ioctl::FIONREAD => {
            drain_input();
            let count = tty.ldisc.lock().read_available() as i32;
            UserPtr::new(arg).write_bytes(vmar, &count.to_ne_bytes())?;
            Ok(0)
        }
        ioctl::TIOCOUTQ => {
            let count: i32 = 0;
            UserPtr::new(arg).write_bytes(vmar, &count.to_ne_bytes())?;
            Ok(0)
        }

        ioctl::TIOCSPGRP
        | ioctl::TIOCGPGRP
        | ioctl::TIOCSCTTY
        | ioctl::TIOCNOTTY
        | ioctl::TIOCGSID => handle_job_control_ioctl(tty, cmd, arg, vmar, proc_opt),

        KDSETMODE | KDGETMODE | KDSKBMODE | KDGKBMODE | VT_SETMODE | VT_GETMODE | VT_GETSTATE
        | VT_ACTIVATE | VT_WAITACTIVE | VT_RELDISP => handle_vt_ioctl(cmd, arg, vmar),

        _ => crate::return_errno!(ENOTTY, "inappropriate ioctl for device"),
    }
}

const KDSETMODE: u32 = 0x4B3A;
const KDGETMODE: u32 = 0x4B3B;
const KDSKBMODE: u32 = 0x4B45;
const KDGKBMODE: u32 = 0x4B44;
const VT_SETMODE: u32 = 0x5602;
const VT_GETMODE: u32 = 0x5601;
const VT_GETSTATE: u32 = 0x5603;
const VT_ACTIVATE: u32 = 0x5606;
const VT_WAITACTIVE: u32 = 0x5607;
const VT_RELDISP: u32 = 0x5605;

fn handle_vt_ioctl(cmd: u32, arg: usize, vmar: &Arc<Vmar>) -> Result<usize> {
    match cmd {
        KDSETMODE => Ok(0),
        KDGETMODE => {
            let mode = 0i32; // KD_TEXT
            UserPtr::new(arg).write_bytes(vmar, &mode.to_ne_bytes())?;
            Ok(0)
        }
        KDSKBMODE => Ok(0),
        KDGKBMODE => {
            let mode = 1i32; // K_XLATE
            UserPtr::new(arg).write_bytes(vmar, &mode.to_ne_bytes())?;
            Ok(0)
        }
        VT_SETMODE => Ok(0),
        VT_GETMODE => {
            let buf = [0u8; 8];
            UserPtr::new(arg).write_bytes(vmar, &buf)?;
            Ok(0)
        }
        VT_GETSTATE => {
            let mut buf = [0u8; 6];
            buf[0..2].copy_from_slice(&1u16.to_ne_bytes());
            buf[4..6].copy_from_slice(&1u16.to_ne_bytes());
            UserPtr::new(arg).write_bytes(vmar, &buf)?;
            Ok(0)
        }
        VT_ACTIVATE | VT_WAITACTIVE | VT_RELDISP => Ok(0),
        _ => crate::return_errno!(ENOTTY, "unsupported VT ioctl"),
    }
}
