// SPDX-License-Identifier: GPL-2.0

//! TTY Line Discipline
//!
//! Provides buffering, canonical editing, and signal generation.

use alloc::collections::VecDeque;
use spin::Mutex;

use ostd::sync::WaitQueue;

use crate::api::ioctl;
use crate::api::termios::{
    winsize, Termios, ECHO, ECHOK, ICANON, ICRNL, IGNCR, INLCR, ISIG, VEOF, VEOL, VERASE, VINTR,
    VKILL, VQUIT, VSUSP,
};
use crate::api::errno::{Errno, Result};
use crate::proc::thread::Thread;
use crate::utils::userspace::user_ptr::UserPtr;
use crate::vm::vmar::Vmar;
use alloc::sync::Arc;

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
    pub wq: WaitQueue,
    // Keyboard and console states
    pub kb_mode: Mutex<u32>,
    pub kd_mode: Mutex<i32>,
    pub led_state: Mutex<u8>,
    pub meta_mode: Mutex<u32>,
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
            wq: WaitQueue::new(),
            kb_mode: Mutex::new(ioctl::K_XLATE),
            kd_mode: Mutex::new(ioctl::KD_TEXT),
            led_state: Mutex::new(0),
            meta_mode: Mutex::new(ioctl::K_METABIT),
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
        if buf.is_empty() {
            return Ok(0);
        }

        let try_read = || -> Option<usize> {
            // Must run before taking any ldisc lock: delivery locks them itself.
            crate::drivers::char::tty::drain_input();
            let mut canon_buf = self.canon_buf.lock();
            let mut state = self.state.lock();

            if state.eof_received {
                state.eof_received = false;
                return Some(0);
            }

            if !canon_buf.is_empty() {
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
                return Some(read);
            }

            None
        };

        Ok(self.wq.wait_until(try_read))
    }

    fn read_raw(&self, buf: &mut [u8]) -> Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        let try_read = || -> Option<usize> {
            crate::drivers::char::tty::drain_input();
            let mut read_buf = self.read_buf.lock();
            let mut state = self.state.lock();

            if state.eof_received {
                state.eof_received = false;
                return Some(0);
            }

            if !read_buf.is_empty() {
                let mut read = 0;
                for b in buf.iter_mut() {
                    if let Some(c) = read_buf.pop_front() {
                        *b = c;
                        read += 1;
                    } else {
                        break;
                    }
                }
                return Some(read);
            }

            None
        };

        Ok(self.wq.wait_until(try_read))
    }

    /// Poll readiness report for character reading/writing
    pub fn poll(&self) -> Result<u32> {
        let termios = self.termios.lock();
        let icanon = (termios.c_lflag & ICANON) != 0;
        drop(termios);

        let mut events = 0x0004 | 0x0100; // POLLOUT | POLLWRNORM
        let has_input = if icanon {
            !self.canon_buf.lock().is_empty() || self.state.lock().eof_received
        } else {
            !self.read_buf.lock().is_empty() || self.state.lock().eof_received
        };

        if has_input {
            events |= 0x0001 | 0x0040; // POLLIN | POLLRDNORM
        }

        Ok(events)
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
                if read_buf.is_empty() {
                    self.state.lock().eof_received = true;
                } else {
                    while let Some(b) = read_buf.pop_front() {
                        canon_buf.push_back(b);
                    }
                }
                drop(canon_buf);
                drop(read_buf);
                self.wq.wake_all();
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
                drop(canon_buf);
                drop(read_buf);
                self.wq.wake_all();
            }
        } else {
            // Raw mode
            let mut read_buf = self.read_buf.lock();
            read_buf.push_back(ch);
            if echo {
                echo_out(ch);
            }
            drop(read_buf);
            self.wq.wake_all();
        }
    }

    /// Primary ioctl dispatcher accepting explicit address space for unit tests and syscalls
    pub fn ioctl_with_vmar(
        &self,
        cmd: u32,
        arg: usize,
        vmar: &Arc<Vmar>,
        proc_opt: Option<&Arc<crate::proc::Proc>>,
    ) -> Result<usize> {
        match cmd {
            ioctl::TCGETS => {
                let termios = *self.termios.lock();
                let bytes = termios.to_bytes();
                UserPtr::new(arg).write_bytes(vmar, &bytes)?;
                Ok(0)
            }
            ioctl::TCSETS | ioctl::TCSETSW | ioctl::TCSETSF => {
                let mut bytes = [0u8; Termios::SIZE];
                UserPtr::new(arg).read_bytes(vmar, &mut bytes)?;
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
                let mut ws = *self.winsize.lock();
                if let Some(cons_lock) = crate::drivers::drm::get_console() {
                    let cons = cons_lock.lock();
                    if cons.cols > 0 && cons.rows > 0 {
                        ws.ws_col = cons.cols as u16;
                        ws.ws_row = cons.rows as u16;
                    }
                }
                if let Some(fb) = crate::drivers::drm::get_framebuffer() {
                    ws.ws_xpixel = fb.width as u16;
                    ws.ws_ypixel = fb.height as u16;
                }
                *self.winsize.lock() = ws;
                let bytes = ws.to_bytes();
                UserPtr::new(arg).write_bytes(vmar, &bytes)?;
                Ok(0)
            }
            ioctl::TIOCSWINSZ => {
                let mut bytes = [0u8; winsize::SIZE];
                UserPtr::new(arg).read_bytes(vmar, &mut bytes)?;
                let new_winsize = winsize::from_bytes(&bytes);
                *self.winsize.lock() = new_winsize;
                let target_pgrp = *self.pgrp.lock();
                let pgrp_opt = target_pgrp.or_else(|| Thread::current_proc().map(|p| p.pgid()));
                if let Some(pgrp) = pgrp_opt {
                    for proc in crate::proc::tree::allproc_iter().filter(|p| p.pgid() == pgrp) {
                        proc.post_signal(crate::api::signal::Signal::SIGWINCH);
                    }
                }
                Ok(0)
            }
            ioctl::FIONREAD => {
                let count = if (self.termios.lock().c_lflag & ICANON) != 0 {
                    self.canon_buf.lock().len()
                } else {
                    self.read_buf.lock().len()
                } as i32;
                UserPtr::new(arg).write_bytes(vmar, &count.to_ne_bytes())?;
                Ok(0)
            }
            ioctl::TIOCSPGRP => {
                let mut pid_bytes = [0u8; 4];
                UserPtr::new(arg).read_bytes(vmar, &mut pid_bytes)?;
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
                let default_pgid = proc_opt.map(|p| p.pgid().as_u32() as i32).unwrap_or(1);
                let pgrp = self.pgrp.lock().map(|p| p.as_u32() as i32).unwrap_or(default_pgid);
                UserPtr::new(arg).write_bytes(vmar, &pgrp.to_ne_bytes())?;
                Ok(0)
            }
            ioctl::TIOCSCTTY => {
                if let Some(proc) = proc_opt {
                    *self.pgrp.lock() = Some(proc.pgid());
                } else {
                    *self.pgrp.lock() = Some(crate::proc::tree::PID_INIT);
                }
                Ok(0)
            }
            ioctl::TIOCNOTTY => {
                *self.pgrp.lock() = None;
                Ok(0)
            }
            ioctl::TIOCGSID => {
                let default_sid = proc_opt.map(|p| p.pgid().as_u32() as i32).unwrap_or(1);
                let sid = self.pgrp.lock().map(|p| p.as_u32() as i32).unwrap_or(default_sid);
                UserPtr::new(arg).write_bytes(vmar, &sid.to_ne_bytes())?;
                Ok(0)
            }
            // ----------------------------------------------------------------
            // Keyboard & Display Ioctls
            // ----------------------------------------------------------------
            ioctl::KDGKBTYPE => {
                let kb_type: u8 = ioctl::KB_101;
                UserPtr::new(arg).write_bytes(vmar, &[kb_type])?;
                Ok(0)
            }
            ioctl::KDGETMODE => {
                let mode = *self.kd_mode.lock();
                UserPtr::new(arg).write_bytes(vmar, &mode.to_ne_bytes())?;
                Ok(0)
            }
            ioctl::KDSETMODE => {
                *self.kd_mode.lock() = arg as i32;
                Ok(0)
            }
            ioctl::KDGKBMODE => {
                let mode = *self.kb_mode.lock() as i32;
                UserPtr::new(arg).write_bytes(vmar, &mode.to_ne_bytes())?;
                Ok(0)
            }
            ioctl::KDSKBMODE => {
                *self.kb_mode.lock() = arg as u32;
                Ok(0)
            }
            ioctl::KDGETLED => {
                let leds = *self.led_state.lock();
                UserPtr::new(arg).write_bytes(vmar, &[leds])?;
                Ok(0)
            }
            ioctl::KDSETLED => {
                *self.led_state.lock() = arg as u8;
                Ok(0)
            }
            ioctl::KDGKBLED => {
                let leds = *self.led_state.lock() as i32;
                UserPtr::new(arg).write_bytes(vmar, &leds.to_ne_bytes())?;
                Ok(0)
            }
            ioctl::KDSKBLED => {
                *self.led_state.lock() = (arg & 0xFF) as u8;
                Ok(0)
            }
            ioctl::KDGKBMETA => {
                let meta = *self.meta_mode.lock() as i32;
                UserPtr::new(arg).write_bytes(vmar, &meta.to_ne_bytes())?;
                Ok(0)
            }
            ioctl::KDSKBMETA => {
                *self.meta_mode.lock() = arg as u32;
                Ok(0)
            }
            ioctl::KDGKBENT => {
                // struct kbentry { kb_table: u8, kb_index: u8, kb_value: u16 }
                let mut entry_bytes = [0u8; 4];
                UserPtr::new(arg).read_bytes(vmar, &mut entry_bytes)?;
                let table = entry_bytes[0];
                let index = entry_bytes[1];
                let val: u16 = if table == 0 { index as u16 } else { 0 };
                entry_bytes[2..4].copy_from_slice(&val.to_ne_bytes());
                UserPtr::new(arg).write_bytes(vmar, &entry_bytes)?;
                Ok(0)
            }
            ioctl::KDSKBENT | ioctl::KDKBDREP => Ok(0),
            ioctl::KDGKBDIACR => {
                // struct kbdiacrs { kb_cnt: u32, kbdiacr: [struct kbdiacr; 256] }
                let zero_cnt: u32 = 0;
                UserPtr::new(arg).write_bytes(vmar, &zero_cnt.to_ne_bytes())?;
                Ok(0)
            }
            ioctl::KDGETKEYCODE | ioctl::KDSETKEYCODE => Ok(0),
            ioctl::GIO_FONT | ioctl::PIO_FONT => Ok(0),
            ioctl::GIO_CMAP | ioctl::PIO_CMAP => Ok(0),
            ioctl::TCSBRK | ioctl::TCXONC => Ok(0),
            // ----------------------------------------------------------------
            // Virtual Console (VT) Ioctls
            // ----------------------------------------------------------------
            ioctl::VT_OPENQRY => {
                let vt: i32 = 1;
                UserPtr::new(arg).write_bytes(vmar, &vt.to_ne_bytes())?;
                Ok(0)
            }
            ioctl::VT_GETSTATE => {
                // struct vt_stat { v_active: u16, v_signal: u16, v_state: u16 }
                let mut stat_bytes = [0u8; 6];
                stat_bytes[0..2].copy_from_slice(&1u16.to_ne_bytes()); // active VT = 1
                stat_bytes[2..4].copy_from_slice(&0u16.to_ne_bytes());
                stat_bytes[4..6].copy_from_slice(&(1u16 << 1).to_ne_bytes()); // state bitmask
                UserPtr::new(arg).write_bytes(vmar, &stat_bytes)?;
                Ok(0)
            }
            ioctl::VT_GETMODE => {
                // struct vt_mode { mode: i8, waitv: i8, relsig: i16, acqsig: i16, frsig: i16 }
                let mode_bytes = [0u8; 8];
                UserPtr::new(arg).write_bytes(vmar, &mode_bytes)?;
                Ok(0)
            }
            ioctl::VT_SETMODE | ioctl::VT_ACTIVATE | ioctl::VT_WAITACTIVE | ioctl::VT_DISALLOCATE => {
                Ok(0)
            }
            ioctl::TIOCLINUX => Ok(0),
            _ => crate::return_errno!(ENOTTY, "inappropriate ioctl for device"),
        }
    }

    /// Ioctl handler
    pub fn ioctl(&self, cmd: u32, arg: usize) -> Result<usize> {
        let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
        let vmar = proc.vmspace();
        self.ioctl_with_vmar(cmd, arg, &vmar, Some(&proc))
    }
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;
    use crate::vm::flags::MmapFlags;
    use crate::vm::perms::VmPerms;

    #[ktest]
    fn keyboard_mode_and_led_ioctls() {
        let ldisc = LineDiscipline::new();
        let vmar = Vmar::new();

        let test_page = 0x0000_2000_0000usize;
        vmar.mmap_anonymous(
            test_page,
            4096,
            VmPerms::READ | VmPerms::WRITE,
            MmapFlags::ANONYMOUS | MmapFlags::PRIVATE,
        )
        .expect("mmap anonymous page");
        vmar.populate_range(&(test_page..test_page + 4096))
            .expect("populate");
        vmar.activate();

        // 1. Test KDGKBTYPE returns KB_101 (0x02)
        ldisc.ioctl_with_vmar(ioctl::KDGKBTYPE, test_page, &vmar, None)
            .expect("KDGKBTYPE");
        let mut kb_type = [0u8; 1];
        UserPtr::new(test_page)
            .read_bytes(&vmar, &mut kb_type)
            .expect("read kb_type");
        assert_eq!(kb_type[0], ioctl::KB_101);

        // 2. Test KDGETMODE and KDSETMODE
        ldisc.ioctl_with_vmar(ioctl::KDGETMODE, test_page, &vmar, None)
            .expect("KDGETMODE");
        let mut kd_mode = [0u8; 4];
        UserPtr::new(test_page)
            .read_bytes(&vmar, &mut kd_mode)
            .expect("read kd_mode");
        assert_eq!(i32::from_ne_bytes(kd_mode), ioctl::KD_TEXT);

        ldisc.ioctl_with_vmar(ioctl::KDSETMODE, ioctl::KD_GRAPHICS as usize, &vmar, None)
            .expect("KDSETMODE");
        ldisc.ioctl_with_vmar(ioctl::KDGETMODE, test_page, &vmar, None)
            .expect("KDGETMODE after set");
        UserPtr::new(test_page)
            .read_bytes(&vmar, &mut kd_mode)
            .expect("read kd_mode");
        assert_eq!(i32::from_ne_bytes(kd_mode), ioctl::KD_GRAPHICS);

        // 3. Test KDGKBMODE and KDSKBMODE
        ldisc.ioctl_with_vmar(ioctl::KDGKBMODE, test_page, &vmar, None)
            .expect("KDGKBMODE");
        let mut kb_mode = [0u8; 4];
        UserPtr::new(test_page)
            .read_bytes(&vmar, &mut kb_mode)
            .expect("read kb_mode");
        assert_eq!(u32::from_ne_bytes(kb_mode), ioctl::K_XLATE);

        ldisc.ioctl_with_vmar(ioctl::KDSKBMODE, ioctl::K_UNICODE as usize, &vmar, None)
            .expect("KDSKBMODE");
        ldisc.ioctl_with_vmar(ioctl::KDGKBMODE, test_page, &vmar, None)
            .expect("KDGKBMODE after set");
        UserPtr::new(test_page)
            .read_bytes(&vmar, &mut kb_mode)
            .expect("read kb_mode");
        assert_eq!(u32::from_ne_bytes(kb_mode), ioctl::K_UNICODE);

        // 4. Test KDGETLED and KDSETLED
        ldisc.ioctl_with_vmar(ioctl::KDGETLED, test_page, &vmar, None)
            .expect("KDGETLED");
        let mut leds = [0u8; 1];
        UserPtr::new(test_page)
            .read_bytes(&vmar, &mut leds)
            .expect("read leds");
        assert_eq!(leds[0], 0);

        let target_leds = ioctl::LED_NUM | ioctl::LED_CAP;
        ldisc.ioctl_with_vmar(ioctl::KDSETLED, target_leds as usize, &vmar, None)
            .expect("KDSETLED");
        ldisc.ioctl_with_vmar(ioctl::KDGETLED, test_page, &vmar, None)
            .expect("KDGETLED after set");
        UserPtr::new(test_page)
            .read_bytes(&vmar, &mut leds)
            .expect("read leds");
        assert_eq!(leds[0], target_leds);

        // 5. Test VT_OPENQRY and VT_GETSTATE
        ldisc.ioctl_with_vmar(ioctl::VT_OPENQRY, test_page, &vmar, None)
            .expect("VT_OPENQRY");
        let mut vt_num = [0u8; 4];
        UserPtr::new(test_page)
            .read_bytes(&vmar, &mut vt_num)
            .expect("read vt_num");
        assert_eq!(i32::from_ne_bytes(vt_num), 1);

        ldisc.ioctl_with_vmar(ioctl::VT_GETSTATE, test_page, &vmar, None)
            .expect("VT_GETSTATE");
        let mut vt_stat = [0u8; 6];
        UserPtr::new(test_page)
            .read_bytes(&vmar, &mut vt_stat)
            .expect("read vt_stat");
        assert_eq!(u16::from_ne_bytes([vt_stat[0], vt_stat[1]]), 1); // v_active = 1
    }

    #[ktest]
    fn ldisc_receive_char_and_poll_readiness() {
        let ldisc = LineDiscipline::new();

        // Initially in canonical mode, empty: poll should report POLLOUT only
        let poll_init = ldisc.poll().expect("poll empty");
        assert_eq!(poll_init & 0x0001, 0, "empty ldisc should not have POLLIN");

        // Feed 'h', 'i', '\n'
        ldisc.receive_char(b'h', &mut |_| {});
        ldisc.receive_char(b'i', &mut |_| {});
        ldisc.receive_char(b'\n', &mut |_| {});

        // Now line is complete: poll should report POLLIN
        let poll_ready = ldisc.poll().expect("poll ready");
        assert_ne!(poll_ready & 0x0001, 0, "ldisc with line must have POLLIN");

        let mut buf = [0u8; 16];
        let n = ldisc.read(&mut buf).expect("read line");
        assert_eq!(n, 3);
        assert_eq!(&buf[..3], b"hi\n");
    }
}
