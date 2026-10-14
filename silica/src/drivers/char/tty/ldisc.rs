// SPDX-License-Identifier: GPL-2.0

//! TTY Line Discipline
//!
//! Provides buffering, canonical editing, and signal generation.

use alloc::collections::VecDeque;
use alloc::sync::Arc;
use spin::Mutex;

use ostd::sync::WaitQueue;

use crate::api::errno::{Errno, Result};
use crate::api::ioctl;
use crate::api::termios::{
    winsize, Termios, ECHO, ECHOK, ICANON, ICRNL, IGNCR, INLCR, ISIG, VEOF, VEOL, VERASE, VINTR,
    VKILL, VQUIT, VSUSP, VWERASE,
};
use crate::fs::poll::PollEvents;
use crate::proc::thread::Thread;
use crate::proc::tree::Pid;
use crate::utils::userspace::user_ptr::UserPtr;
use crate::vm::vmar::Vmar;

/// Maximum line length retained in canonical line editing buffer before Enter
const MAX_CANON: usize = 4096;

/// Internal character buffers for line discipline queues
#[derive(Debug, Default)]
struct LdiscBuffers {
    /// Line currently being edited in canonical mode before delimiter
    line_buf: VecDeque<u8>,
    /// Committed characters ready to be returned by `read()`
    read_queue: VecDeque<u8>,
    /// Set when an EOF character is received on an empty line
    eof_received: bool,
}

impl LdiscBuffers {
    fn new() -> Self {
        Self {
            line_buf: VecDeque::with_capacity(128),
            read_queue: VecDeque::with_capacity(4096),
            eof_received: false,
        }
    }

    fn clear(&mut self) {
        self.line_buf.clear();
        self.read_queue.clear();
        self.eof_received = false;
    }

    fn commit_line(&mut self) {
        while let Some(b) = self.line_buf.pop_front() {
            self.read_queue.push_back(b);
        }
    }
}

/// Keyboard and console display hardware states
#[derive(Debug, Clone, Copy)]
struct KeyboardConsoleState {
    kb_mode: u32,
    kd_mode: i32,
    led_state: u8,
    meta_mode: u32,
}

impl KeyboardConsoleState {
    const fn new() -> Self {
        Self {
            kb_mode: ioctl::K_XLATE,
            kd_mode: ioctl::KD_TEXT,
            led_state: 0,
            meta_mode: ioctl::K_METABIT,
        }
    }
}

/// A TTY Line Discipline instance
pub struct LineDiscipline {
    pub termios: Mutex<Termios>,
    pub winsize: Mutex<winsize>,
    buffers: Mutex<LdiscBuffers>,
    pgrp: Mutex<Option<Pid>>,
    pub wq: WaitQueue,
    kb_state: Mutex<KeyboardConsoleState>,
}

impl LineDiscipline {
    pub fn new() -> Self {
        let (rows, cols, xpix, ypix) = if let Some(cons_lock) = crate::drivers::drm::get_console() {
            let cons = cons_lock.lock();
            let fb = crate::drivers::drm::get_framebuffer();
            let (w, h) = fb.map(|f| (f.width as u16, f.height as u16)).unwrap_or((1280, 800));
            (cons.rows as u16, cons.cols as u16, w, h)
        } else {
            (50, 160, 1280, 800)
        };

        Self {
            termios: Mutex::new(Termios::default()),
            winsize: Mutex::new(winsize {
                ws_row: rows,
                ws_col: cols,
                ws_xpixel: xpix,
                ws_ypixel: ypix,
            }),
            buffers: Mutex::new(LdiscBuffers::new()),
            pgrp: Mutex::new(None),
            wq: WaitQueue::new(),
            kb_state: Mutex::new(KeyboardConsoleState::new()),
        }
    }

    /// Returns `(OPOST, ONLCR)` output processing flags.
    pub fn output_flags(&self) -> (bool, bool) {
        let termios = self.termios.lock();
        (
            (termios.c_oflag & crate::api::termios::OPOST) != 0,
            (termios.c_oflag & crate::api::termios::ONLCR) != 0,
        )
    }

    /// Wakes all sleeping readers and pollers.
    pub fn wake_readers(&self) {
        self.wq.wake_all();
        crate::fs::poll::notify();
    }

    /// Delivers a signal to the foreground process group.
    fn signal_pgrp(&self, sig: crate::api::signal::Signal) {
        let target_pgrp = *self.pgrp.lock();
        let pgrp_opt = target_pgrp.or_else(|| Thread::current_proc().map(|p| p.pgid()));
        if let Some(pgrp) = pgrp_opt {
            for proc in crate::proc::tree::allproc_iter().filter(|p| p.pgid() == pgrp) {
                proc.post_signal(sig);
            }
        }
    }

    /// Read data from the line discipline buffer.
    pub fn read(&self, buf: &mut [u8]) -> Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        let termios = *self.termios.lock();
        let icanon = (termios.c_lflag & ICANON) != 0;
        let veol = termios.c_cc[VEOL];
        let vmin = termios.c_cc[crate::api::termios::VMIN];

        let try_read = || -> Option<usize> {
            crate::drivers::char::tty::drain_input();
            let mut buffers = self.buffers.lock();

            if !buffers.read_queue.is_empty() {
                let mut read = 0;
                for b in buf.iter_mut() {
                    if let Some(c) = buffers.read_queue.pop_front() {
                        *b = c;
                        read += 1;
                        if icanon && (c == b'\n' || (veol != 0 && c == veol)) {
                            break;
                        }
                    } else {
                        break;
                    }
                }
                return Some(read);
            }

            // Return EOF only after all queued input has been consumed
            if buffers.eof_received {
                buffers.eof_received = false;
                return Some(0);
            }

            // Non-canonical mode with VMIN=0: non-blocking immediate return
            if !icanon && vmin == 0 {
                return Some(0);
            }

            None
        };

        Ok(self.wq.wait_until(try_read))
    }

    /// Poll readiness report for character reading/writing
    pub fn poll(&self) -> Result<u32> {
        let mut events = (PollEvents::OUT | PollEvents::WRNORM).bits() as u32;

        let buffers = self.buffers.lock();
        let has_input = !buffers.read_queue.is_empty() || buffers.eof_received;
        drop(buffers);

        if has_input {
            events |= (PollEvents::IN | PollEvents::RDNORM).bits() as u32;
        }

        Ok(events)
    }

    /// Receive a character from the hardware driver
    pub fn receive_char(&self, mut ch: u8, echo_out: &mut impl FnMut(&[u8])) {
        let termios = *self.termios.lock();
        let lflag = termios.c_lflag;
        let iflag = termios.c_iflag;
        let cc = termios.c_cc;

        // Input processing (ISTRIP, IGNCR, ICRNL, INLCR)
        if (iflag & crate::api::termios::ISTRIP) != 0 {
            ch &= 0x7F;
        }

        if ch == b'\r' {
            if (iflag & IGNCR) != 0 {
                return;
            }
            if (iflag & ICRNL) != 0 {
                ch = b'\n';
            }
        } else if ch == b'\n' && (iflag & INLCR) != 0 {
            ch = b'\r';
        }

        let isig = (lflag & ISIG) != 0;
        let icanon = (lflag & ICANON) != 0;
        let echo = (lflag & ECHO) != 0;

        // Signal generation
        if isig {
            if ch == cc[VINTR] {
                self.signal_pgrp(crate::api::signal::Signal::SIGINT);
                if (lflag & crate::api::termios::NOFLSH) == 0 {
                    self.buffers.lock().clear();
                }
                if echo {
                    echo_out(b"^C");
                }
                return;
            }
            if ch == cc[VQUIT] {
                self.signal_pgrp(crate::api::signal::Signal::SIGQUIT);
                if (lflag & crate::api::termios::NOFLSH) == 0 {
                    self.buffers.lock().clear();
                }
                if echo {
                    echo_out(b"^\\");
                }
                return;
            }
            if ch == cc[VSUSP] {
                self.signal_pgrp(crate::api::signal::Signal::SIGTSTP);
                if echo {
                    echo_out(b"^Z");
                }
                return;
            }
        }

        if icanon {
            let mut buffers = self.buffers.lock();

            if ch == cc[VEOF] {
                if buffers.line_buf.is_empty() {
                    buffers.eof_received = true;
                } else {
                    buffers.commit_line();
                }
                drop(buffers);
                self.wake_readers();
                return;
            }

            if ch == cc[VERASE] || ch == 8 || ch == 127 {
                // Backspace or DEL
                if buffers.line_buf.pop_back().is_some() && echo {
                    echo_out(b"\x08 \x08");
                }
                return;
            }

            if ch == cc[VWERASE] && ch != 0 {
                // Word erase (Ctrl+W)
                let mut erased = 0;
                while let Some(&last) = buffers.line_buf.back() {
                    if last == b' ' || last == b'\t' {
                        buffers.line_buf.pop_back();
                        erased += 1;
                    } else {
                        break;
                    }
                }
                while let Some(&last) = buffers.line_buf.back() {
                    if last != b' ' && last != b'\t' {
                        buffers.line_buf.pop_back();
                        erased += 1;
                    } else {
                        break;
                    }
                }
                if echo {
                    for _ in 0..erased {
                        echo_out(b"\x08 \x08");
                    }
                }
                return;
            }

            if ch == cc[VKILL] {
                let count = buffers.line_buf.len();
                buffers.line_buf.clear();
                if echo && (lflag & ECHOK) != 0 {
                    for _ in 0..count {
                        echo_out(b"\x08 \x08");
                    }
                }
                return;
            }

            // Normal character
            if buffers.line_buf.len() < MAX_CANON {
                buffers.line_buf.push_back(ch);
                if echo {
                    echo_out(&[ch]);
                }
            }

            if ch == b'\n' || (cc[VEOL] != 0 && ch == cc[VEOL]) {
                // Line complete: commit to read queue
                buffers.commit_line();
                drop(buffers);
                self.wake_readers();
            }
        } else {
            // Raw mode
            let mut buffers = self.buffers.lock();
            buffers.read_queue.push_back(ch);
            if echo {
                echo_out(&[ch]);
            }
            drop(buffers);
            self.wake_readers();
        }
    }

    fn handle_termios_ioctl(&self, cmd: u32, arg: usize, vmar: &Arc<Vmar>) -> Result<usize> {
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
                    self.buffers.lock().clear();
                }
                Ok(0)
            }
            ioctl::TCFLSH => {
                if arg == 0 || arg == 2 {
                    self.buffers.lock().clear();
                }
                Ok(0)
            }
            ioctl::TCSBRK | ioctl::TCSBRKP | ioctl::TIOCSBRK | ioctl::TIOCCBRK => {
                // Break generation / drain wait: no-op on virtual console
                Ok(0)
            }
            ioctl::TCXONC => {
                // Flow control: accept and succeed
                Ok(0)
            }
            _ => unreachable!(),
        }
    }

    fn handle_winsize_ioctl(&self, cmd: u32, arg: usize, vmar: &Arc<Vmar>) -> Result<usize> {
        match cmd {
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
                if ws.ws_col == 0 {
                    ws.ws_col = 80;
                }
                if ws.ws_row == 0 {
                    ws.ws_row = 25;
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
                self.signal_pgrp(crate::api::signal::Signal::SIGWINCH);
                Ok(0)
            }
            _ => unreachable!(),
        }
    }

    fn handle_job_control_ioctl(
        &self,
        cmd: u32,
        arg: usize,
        vmar: &Arc<Vmar>,
        proc_opt: Option<&Arc<crate::proc::Proc>>,
    ) -> Result<usize> {
        match cmd {
            ioctl::TIOCSPGRP => {
                let mut pid_bytes = [0u8; 4];
                UserPtr::new(arg).read_bytes(vmar, &mut pid_bytes)?;
                let raw_pid = i32::from_ne_bytes(pid_bytes);
                if raw_pid <= 0 {
                    crate::return_errno!(EINVAL, "invalid process group ID");
                }
                let pid = Pid::from_u32(raw_pid as u32).ok_or(Errno::EINVAL)?;
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
                let pgid = proc_opt.map(|p| p.pgid()).unwrap_or(crate::proc::tree::PID_INIT);
                *self.pgrp.lock() = Some(pgid);
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
            _ => unreachable!(),
        }
    }

    fn handle_keyboard_display_ioctl(
        &self,
        cmd: u32,
        arg: usize,
        vmar: &Arc<Vmar>,
    ) -> Result<usize> {
        let mut kb = self.kb_state.lock();
        match cmd {
            ioctl::KDGKBTYPE => {
                let kb_type: u8 = ioctl::KB_101;
                UserPtr::new(arg).write_bytes(vmar, &[kb_type])?;
                Ok(0)
            }
            ioctl::KDGETMODE => {
                let mode = kb.kd_mode;
                UserPtr::new(arg).write_bytes(vmar, &mode.to_ne_bytes())?;
                Ok(0)
            }
            ioctl::KDSETMODE => {
                kb.kd_mode = arg as i32;
                Ok(0)
            }
            ioctl::KDGKBMODE => {
                let mode = kb.kb_mode as i32;
                UserPtr::new(arg).write_bytes(vmar, &mode.to_ne_bytes())?;
                Ok(0)
            }
            ioctl::KDSKBMODE => {
                kb.kb_mode = arg as u32;
                Ok(0)
            }
            ioctl::KDGETLED => {
                let leds = kb.led_state;
                UserPtr::new(arg).write_bytes(vmar, &[leds])?;
                Ok(0)
            }
            ioctl::KDSETLED => {
                kb.led_state = arg as u8;
                Ok(0)
            }
            ioctl::KDGKBLED => {
                let leds = kb.led_state as i32;
                UserPtr::new(arg).write_bytes(vmar, &leds.to_ne_bytes())?;
                Ok(0)
            }
            ioctl::KDSKBLED => {
                kb.led_state = (arg & 0xFF) as u8;
                Ok(0)
            }
            ioctl::KDGKBMETA => {
                let meta = kb.meta_mode as i32;
                UserPtr::new(arg).write_bytes(vmar, &meta.to_ne_bytes())?;
                Ok(0)
            }
            ioctl::KDSKBMETA => {
                kb.meta_mode = arg as u32;
                Ok(0)
            }
            ioctl::KDGKBENT => {
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
                let zero_cnt: u32 = 0;
                UserPtr::new(arg).write_bytes(vmar, &zero_cnt.to_ne_bytes())?;
                Ok(0)
            }
            ioctl::KDGETKEYCODE | ioctl::KDSETKEYCODE => Ok(0),
            ioctl::GIO_FONT | ioctl::PIO_FONT => Ok(0),
            ioctl::GIO_CMAP | ioctl::PIO_CMAP => Ok(0),
            _ => unreachable!(),
        }
    }

    fn handle_vt_ioctl(&self, cmd: u32, arg: usize, vmar: &Arc<Vmar>) -> Result<usize> {
        match cmd {
            ioctl::VT_OPENQRY => {
                let vt: i32 = 1;
                UserPtr::new(arg).write_bytes(vmar, &vt.to_ne_bytes())?;
                Ok(0)
            }
            ioctl::VT_GETSTATE => {
                let mut stat_bytes = [0u8; 6];
                stat_bytes[0..2].copy_from_slice(&1u16.to_ne_bytes()); // active VT = 1
                stat_bytes[2..4].copy_from_slice(&0u16.to_ne_bytes());
                stat_bytes[4..6].copy_from_slice(&(1u16 << 1).to_ne_bytes()); // state bitmask
                UserPtr::new(arg).write_bytes(vmar, &stat_bytes)?;
                Ok(0)
            }
            ioctl::VT_GETMODE => {
                let mode_bytes = [0u8; 8];
                UserPtr::new(arg).write_bytes(vmar, &mode_bytes)?;
                Ok(0)
            }
            ioctl::VT_SETMODE
            | ioctl::VT_ACTIVATE
            | ioctl::VT_WAITACTIVE
            | ioctl::VT_DISALLOCATE
            | ioctl::TIOCLINUX => Ok(0),
            _ => unreachable!(),
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
            ioctl::TCGETS
            | ioctl::TCSETS
            | ioctl::TCSETSW
            | ioctl::TCSETSF
            | ioctl::TCFLSH
            | ioctl::TCSBRK
            | ioctl::TCSBRKP
            | ioctl::TIOCSBRK
            | ioctl::TIOCCBRK
            | ioctl::TCXONC => self.handle_termios_ioctl(cmd, arg, vmar),
            ioctl::TIOCGWINSZ | ioctl::TIOCSWINSZ => self.handle_winsize_ioctl(cmd, arg, vmar),
            ioctl::FIONREAD => {
                let count = self.buffers.lock().read_queue.len() as i32;
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
            | ioctl::TIOCGSID => self.handle_job_control_ioctl(cmd, arg, vmar, proc_opt),
            ioctl::KDGKBTYPE
            | ioctl::KDGETMODE
            | ioctl::KDSETMODE
            | ioctl::KDGKBMODE
            | ioctl::KDSKBMODE
            | ioctl::KDGETLED
            | ioctl::KDSETLED
            | ioctl::KDGKBLED
            | ioctl::KDSKBLED
            | ioctl::KDGKBMETA
            | ioctl::KDSKBMETA
            | ioctl::KDGKBENT
            | ioctl::KDSKBENT
            | ioctl::KDKBDREP
            | ioctl::KDGKBDIACR
            | ioctl::KDGETKEYCODE
            | ioctl::KDSETKEYCODE
            | ioctl::GIO_FONT
            | ioctl::PIO_FONT
            | ioctl::GIO_CMAP
            | ioctl::PIO_CMAP => self.handle_keyboard_display_ioctl(cmd, arg, vmar),
            ioctl::VT_OPENQRY
            | ioctl::VT_GETSTATE
            | ioctl::VT_GETMODE
            | ioctl::VT_SETMODE
            | ioctl::VT_ACTIVATE
            | ioctl::VT_WAITACTIVE
            | ioctl::VT_DISALLOCATE
            | ioctl::TIOCLINUX => self.handle_vt_ioctl(cmd, arg, vmar),
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

impl Default for LineDiscipline {
    fn default() -> Self {
        Self::new()
    }
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(ktest)]
mod tests {
    use super::*;
    use crate::vm::flags::MmapFlags;
    use crate::vm::perms::VmPerms;
    use ostd::prelude::ktest;

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

    #[ktest]
    fn ldisc_canonical_editing_and_word_erase() {
        let ldisc = LineDiscipline::new();

        // Feed 'a', 'b', 'c', backspace, 'd', '\n' -> "abd\n"
        ldisc.receive_char(b'a', &mut |_| {});
        ldisc.receive_char(b'b', &mut |_| {});
        ldisc.receive_char(b'c', &mut |_| {});
        ldisc.receive_char(0x7F, &mut |_| {}); // DEL/Backspace
        ldisc.receive_char(b'd', &mut |_| {});
        ldisc.receive_char(b'\n', &mut |_| {});

        let mut buf = [0u8; 16];
        let n = ldisc.read(&mut buf).expect("read edited line");
        assert_eq!(n, 4);
        assert_eq!(&buf[..4], b"abd\n");

        // Test Word Erase (Ctrl+W = 0x17): feed "hi yo^W\n" -> "hi \n"
        ldisc.receive_char(b'h', &mut |_| {});
        ldisc.receive_char(b'i', &mut |_| {});
        ldisc.receive_char(b' ', &mut |_| {});
        ldisc.receive_char(b'y', &mut |_| {});
        ldisc.receive_char(b'o', &mut |_| {});
        ldisc.receive_char(0x17, &mut |_| {}); // Ctrl+W
        ldisc.receive_char(b'\n', &mut |_| {});

        let n = ldisc.read(&mut buf).expect("read line after word erase");
        assert_eq!(n, 4);
        assert_eq!(&buf[..4], b"hi \n");
    }

    #[ktest]
    fn tiocwinsz_and_tiocoutq_and_break_ioctls() {
        let ldisc = LineDiscipline::new();
        let vmar = Vmar::new();

        let test_page = 0x0000_3000_0000usize;
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

        // 1. TIOCGWINSZ should return valid dimensions (at least 80x25 fallback)
        ldisc.ioctl_with_vmar(ioctl::TIOCGWINSZ, test_page, &vmar, None)
            .expect("TIOCGWINSZ");
        let mut ws_bytes = [0u8; winsize::SIZE];
        UserPtr::new(test_page)
            .read_bytes(&vmar, &mut ws_bytes)
            .expect("read winsize");
        let ws = winsize::from_bytes(&ws_bytes);
        assert!(ws.ws_col >= 80, "ws_col must be at least 80");
        assert!(ws.ws_row >= 25, "ws_row must be at least 25");

        // 2. TIOCOUTQ should return 0 (no unsent buffered bytes)
        ldisc.ioctl_with_vmar(ioctl::TIOCOUTQ, test_page, &vmar, None)
            .expect("TIOCOUTQ");
        let mut outq_bytes = [0u8; 4];
        UserPtr::new(test_page)
            .read_bytes(&vmar, &mut outq_bytes)
            .expect("read TIOCOUTQ");
        assert_eq!(i32::from_ne_bytes(outq_bytes), 0);

        // 3. TCSBRK, TCSBRKP, TIOCSBRK, TIOCCBRK, TCXONC must succeed
        ldisc.ioctl_with_vmar(ioctl::TCSBRK, 0, &vmar, None).expect("TCSBRK");
        ldisc.ioctl_with_vmar(ioctl::TCSBRKP, 0, &vmar, None).expect("TCSBRKP");
        ldisc.ioctl_with_vmar(ioctl::TIOCSBRK, 0, &vmar, None).expect("TIOCSBRK");
        ldisc.ioctl_with_vmar(ioctl::TIOCCBRK, 0, &vmar, None).expect("TIOCCBRK");
        ldisc.ioctl_with_vmar(ioctl::TCXONC, 1, &vmar, None).expect("TCXONC");

        // 4. FIONREAD should report available queued characters
        ldisc.receive_char(b'o', &mut |_| {});
        ldisc.receive_char(b'k', &mut |_| {});
        ldisc.receive_char(b'\n', &mut |_| {});

        ldisc.ioctl_with_vmar(ioctl::FIONREAD, test_page, &vmar, None)
            .expect("FIONREAD");
        let mut inq_bytes = [0u8; 4];
        UserPtr::new(test_page)
            .read_bytes(&vmar, &mut inq_bytes)
            .expect("read FIONREAD");
        assert_eq!(i32::from_ne_bytes(inq_bytes), 3);
    }
}
