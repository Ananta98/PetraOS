// SPDX-License-Identifier: GPL-2.0

//! TTY Subsystem.
//!
//! Based on Zinnia TTY subsystem design and POSIX/Linux TTY standards.
//! Integrates line discipline, termios, session job control, pseudo-terminals,
//! and hardware display drivers.

pub mod console;
pub mod ldisc;
pub mod pty;

use alloc::{
    boxed::Box,
    collections::btree_map::BTreeMap,
    string::String,
    sync::Arc,
    vec::Vec,
};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use spin::Mutex;

use ostd::sync::{LocalIrqDisabled, SpinLock, WaitQueue};

use crate::api::errno::{Errno, Result};
use crate::api::ioctl;
use crate::api::signal::Signal;
use crate::api::termios::{
    winsize, Termios, CS8, CREAD, ECHO, ECHOE, ECHOK, ECHONL, ICANON, ICRNL, IEXTEN, ISIG, IXON,
    ONLCR, OPOST, VDISCARD, VEOF, VEOL, VEOL2, VERASE, VINTR, VKILL, VLNEXT, VMIN, VQUIT,
    VREPRINT, VSTART, VSTOP, VSUSP, VTIME, VWERASE,
};
use crate::drivers::{register_chrdev, CharacterDevice};
use crate::fs::poll::PollEvents;
use crate::fs::vfs::{File, FileOps, INode, Mode, OpenFlags};
use crate::proc::thread::Thread;
use crate::proc::tree::Pid;
use crate::utils::lockless_queue::LocklessQueue;
use crate::utils::userspace::user_ptr::UserPtr;
use crate::vm::vmar::Vmar;

use console::FramebufferConsoleDriver;
use ldisc::LineDiscipline;

/// Hardware or virtual driver backing a TTY.
pub trait TtyDriver: Send + Sync {
    /// Writes pre-processed output data to the device surface.
    fn write_output(&self, data: &[u8]) -> Result<()>;

    /// Flushes any pending output to the hardware.
    fn flush(&self) -> Result<()> {
        Ok(())
    }

    /// Sets the window geometry on the hardware device.
    fn set_winsize(&self, _ws: winsize) -> Result<()> {
        Ok(())
    }

    /// Returns the window size in rows, columns, and pixels.
    fn get_winsize(&self) -> winsize {
        winsize {
            ws_row: 25,
            ws_col: 80,
            ws_xpixel: 0,
            ws_ypixel: 0,
        }
    }
}

/// Generates the standard default termios configuration matching Zinnia / Linux.
pub fn default_termios() -> Termios {
    let mut t = Termios::default();
    t.c_iflag = ICRNL | IXON;
    t.c_oflag = OPOST | ONLCR;
    t.c_cflag = CS8 | CREAD;
    t.c_lflag = ISIG | ICANON | ECHO | ECHOE | ECHOK | ECHONL | IEXTEN;

    t.c_cc[VINTR] = 0x03; // ^C
    t.c_cc[VQUIT] = 0x1C; // ^\
    t.c_cc[VERASE] = 0x7F; // DEL
    t.c_cc[VKILL] = 0x15; // ^U
    t.c_cc[VEOF] = 0x04; // ^D
    t.c_cc[VTIME] = 0;
    t.c_cc[VMIN] = 1;
    t.c_cc[VSTART] = 0x11; // ^Q
    t.c_cc[VSTOP] = 0x13; // ^S
    t.c_cc[VSUSP] = 0x1A; // ^Z
    t.c_cc[VEOL] = 0;
    t.c_cc[VREPRINT] = 0x12; // ^R
    t.c_cc[VDISCARD] = 0x0F; // ^O
    t.c_cc[VWERASE] = 0x17; // ^W
    t.c_cc[VLNEXT] = 0x16; // ^V
    t.c_cc[VEOL2] = 0;
    t
}

static TTY_INDEX: AtomicU32 = AtomicU32::new(0);
static TTYS: Mutex<BTreeMap<u32, Arc<Tty>>> = Mutex::new(BTreeMap::new());

/// High-level TTY instance managing line discipline, job control, and hardware driver.
pub struct Tty {
    pub name: String,
    pub index: u32,
    pub ldisc: Mutex<LineDiscipline>,
    pub driver: Arc<dyn TtyDriver>,
    pub winsize: Mutex<winsize>,
    pub winsize_user_set: AtomicBool,
    pub foreground_pgrp: Mutex<Option<Pid>>,
    pub session: Mutex<Option<Pid>>,
    pub wq: WaitQueue,
    pub hangup: AtomicBool,
}

impl Tty {
    /// Constructs a new TTY backed by the specified driver and registers it in the global table.
    pub fn new(name: String, driver: Arc<dyn TtyDriver>) -> Arc<Self> {
        let ws = driver.get_winsize();
        let index = TTY_INDEX.fetch_add(1, Ordering::Relaxed);
        let tty = Arc::new(Self {
            name,
            index,
            ldisc: Mutex::new(LineDiscipline::new()),
            driver,
            winsize: Mutex::new(ws),
            winsize_user_set: AtomicBool::new(false),
            foreground_pgrp: Mutex::new(None),
            session: Mutex::new(None),
            wq: WaitQueue::new(),
            hangup: AtomicBool::new(false),
        });
        TTYS.lock().insert(index, Arc::clone(&tty));
        tty
    }

    /// Marks the TTY as hung up and wakes all pending waiters.
    pub fn hangup(&self) {
        self.hangup.store(true, Ordering::Release);
        self.wq.wake_all();
    }

    /// Returns whether the TTY has been hung up.
    pub fn is_hung_up(&self) -> bool {
        self.hangup.load(Ordering::Acquire)
    }

    /// Detaches this TTY from its controlling session.
    pub fn detach_session(self: &Arc<Self>) {
        let _ = self.session.lock().take();
        *self.foreground_pgrp.lock() = None;
    }

    /// Sends a signal to the foreground process group.
    pub fn signal_foreground(&self, sig: Signal) {
        let target_pgrp = *self.foreground_pgrp.lock();
        let pgrp_opt = target_pgrp.or_else(|| Thread::current_proc().map(|p| p.pgid()));
        if let Some(pgrp) = pgrp_opt {
            for proc in crate::proc::tree::allproc_iter().filter(|p| p.pgid() == pgrp) {
                proc.post_signal(sig);
            }
        }
    }

    /// POSIX job control gate checking background reads/writes.
    pub fn job_control_gate(self: &Arc<Self>, is_read: bool) -> Result<()> {
        let Some(proc) = Thread::current_proc() else {
            return Ok(());
        };

        let pgrp = proc.pgid();
        let fg_opt = *self.foreground_pgrp.lock();

        if let Some(fg) = fg_opt {
            if fg != pgrp {
                let sig = if is_read { Signal::SIGTTIN } else { Signal::SIGTTOU };
                proc.post_signal(sig);
                return Err(Errno::EINTR);
            }
        }
        Ok(())
    }

    /// Wakes up threads waiting for input data.
    pub fn wake_readers(&self) {
        self.wq.wake_all();
    }

    /// Feeds a raw byte into the line discipline and writes any resulting echo to the driver.
    pub fn input_byte(&self, byte: u8) {
        let output = self.input_byte_internal(byte);
        if !output.is_empty() {
            let _ = self.driver.write_output(&output);
        }
    }

    /// Feeds a byte into the line discipline, returning any echo output.
    pub fn input_byte_internal(&self, byte: u8) -> Vec<u8> {
        let out = self.ldisc.lock().input_char(byte, self);
        self.wake_readers();
        out
    }

    /// Reads data from the TTY buffer into `buf`.
    pub fn read(&self, buf: &mut [u8]) -> Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        drain_input();

        let (canon, vmin, vtime) = {
            let ldisc = self.ldisc.lock();
            (
                ldisc.is_canon(),
                ldisc.cc(VMIN) as usize,
                ldisc.cc(VTIME) as usize,
            )
        };

        let try_read = || -> Option<usize> {
            drain_input();
            let mut ldisc = self.ldisc.lock();
            let avail = ldisc.read_available();

            if canon {
                if avail > 0 {
                    return Some(ldisc.read_into(buf));
                }
                if ldisc.eof_received {
                    ldisc.eof_received = false;
                    return Some(0);
                }
                if self.is_hung_up() {
                    return Some(0);
                }
                None
            } else {
                if vmin == 0 {
                    if avail > 0 {
                        Some(ldisc.read_into(buf))
                    } else {
                        Some(0)
                    }
                } else if avail > 0 && (avail >= vmin.min(buf.len()) || vtime > 0) {
                    Some(ldisc.read_into(buf))
                } else if self.is_hung_up() {
                    Some(ldisc.read_into(buf))
                } else {
                    None
                }
            }
        };

        Ok(self.wq.wait_until(try_read))
    }

    /// Writes data through the line discipline to the backing driver.
    pub fn write(&self, buf: &[u8]) -> Result<usize> {
        if self.is_hung_up() {
            return Err(Errno::EIO);
        }

        let output = self.ldisc.lock().write_output(buf);
        self.driver.write_output(&output)?;
        Ok(buf.len())
    }

    /// Reports poll readiness for reading/writing.
    pub fn poll(&self) -> Result<u32> {
        drain_input();
        let mut events = (PollEvents::OUT | PollEvents::WRNORM).bits() as u32;
        let ldisc = self.ldisc.lock();
        if ldisc.read_available() > 0 || ldisc.eof_received {
            events |= (PollEvents::IN | PollEvents::RDNORM).bits() as u32;
        }
        Ok(events)
    }

    // ------------------------------------------------------------------------
    // Modular ioctl Handlers
    // ------------------------------------------------------------------------

    fn handle_termios_ioctl(&self, cmd: u32, arg: usize, vmar: &Arc<Vmar>) -> Result<usize> {
        match cmd {
            ioctl::TCGETS => {
                let termios = self.ldisc.lock().termios;
                let bytes = termios.to_bytes();
                UserPtr::new(arg).write_bytes(vmar, &bytes)?;
                Ok(0)
            }
            ioctl::TCSETS | ioctl::TCSETSW | ioctl::TCSETSF => {
                let mut bytes = [0u8; Termios::SIZE];
                UserPtr::new(arg).read_bytes(vmar, &mut bytes)?;
                let new_termios = Termios::from_bytes(&bytes);
                let mut ldisc = self.ldisc.lock();
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
                let mut ldisc = self.ldisc.lock();
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

    fn handle_winsize_ioctl(&self, cmd: u32, arg: usize, vmar: &Arc<Vmar>) -> Result<usize> {
        match cmd {
            ioctl::TIOCGWINSZ => {
                let dws = self.driver.get_winsize();
                let mut ws = *self.winsize.lock();
                if dws.ws_col > 0 && dws.ws_row > 0 {
                    if !self.winsize_user_set.load(Ordering::Acquire) || ws.ws_col == 0 || ws.ws_row == 0 {
                        ws = dws;
                        *self.winsize.lock() = ws;
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
                *self.winsize.lock() = ws;
                self.winsize_user_set.store(true, Ordering::Release);
                let _ = self.driver.set_winsize(ws);
                self.signal_foreground(Signal::SIGWINCH);
                Ok(0)
            }
            _ => crate::return_errno!(ENOTTY, "unsupported winsize ioctl"),
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
                let mut bytes = [0u8; 4];
                UserPtr::new(arg).read_bytes(vmar, &mut bytes)?;
                let raw = u32::from_ne_bytes(bytes);
                let pgrp = Pid::from_u32(raw).ok_or(Errno::EINVAL)?;
                *self.foreground_pgrp.lock() = Some(pgrp);
                Ok(0)
            }
            ioctl::TIOCGPGRP => {
                let pgrp = self
                    .foreground_pgrp
                    .lock()
                    .or_else(|| proc_opt.map(|p| p.pgid()))
                    .unwrap_or(crate::proc::tree::PID_INIT);
                let bytes = pgrp.as_u32().to_ne_bytes();
                UserPtr::new(arg).write_bytes(vmar, &bytes)?;
                Ok(0)
            }
            ioctl::TIOCSCTTY => {
                let proc = proc_opt.ok_or(Errno::EPERM)?;
                *self.session.lock() = Some(proc.pid());
                *self.foreground_pgrp.lock() = Some(proc.pgid());
                Ok(0)
            }
            ioctl::TIOCNOTTY => {
                *self.session.lock() = None;
                *self.foreground_pgrp.lock() = None;
                Ok(0)
            }
            ioctl::TIOCGSID => {
                let sid = self
                    .session
                    .lock()
                    .or_else(|| proc_opt.map(|p| p.pid()))
                    .unwrap_or(crate::proc::tree::PID_INIT);
                let bytes = sid.as_u32().to_ne_bytes();
                UserPtr::new(arg).write_bytes(vmar, &bytes)?;
                Ok(0)
            }
            _ => crate::return_errno!(ENOTTY, "unsupported job control ioctl"),
        }
    }

    fn handle_keyboard_display_ioctl(&self, cmd: u32, arg: usize, vmar: &Arc<Vmar>) -> Result<usize> {
        match cmd {
            ioctl::KDGKBTYPE => {
                let kb_type = [ioctl::KB_101];
                UserPtr::new(arg).write_bytes(vmar, &kb_type)?;
                Ok(0)
            }
            ioctl::KDGETMODE => {
                let mode: i32 = ioctl::KD_TEXT;
                UserPtr::new(arg).write_bytes(vmar, &mode.to_ne_bytes())?;
                Ok(0)
            }
            ioctl::KDSETMODE => Ok(0),
            ioctl::KDGKBMODE => {
                let k_mode: u32 = ioctl::K_XLATE;
                UserPtr::new(arg).write_bytes(vmar, &k_mode.to_ne_bytes())?;
                Ok(0)
            }
            ioctl::KDSKBMODE => Ok(0),
            ioctl::KDGETLED | ioctl::KDGKBLED => {
                let leds: u8 = 0;
                UserPtr::new(arg).write_bytes(vmar, &[leds])?;
                Ok(0)
            }
            ioctl::KDSETLED | ioctl::KDSKBLED => Ok(0),
            ioctl::KDGKBMETA => {
                let meta: u32 = ioctl::K_METABIT;
                UserPtr::new(arg).write_bytes(vmar, &meta.to_ne_bytes())?;
                Ok(0)
            }
            ioctl::KDSKBMETA => Ok(0),
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
            ioctl::KDSKBENT | ioctl::KDKBDREP | ioctl::KDGKBDIACR
            | ioctl::KDGETKEYCODE | ioctl::KDSETKEYCODE => Ok(0),
            ioctl::GIO_FONT | ioctl::PIO_FONT | ioctl::GIO_CMAP | ioctl::PIO_CMAP => Ok(0),
            _ => crate::return_errno!(ENOTTY, "unsupported keyboard display ioctl"),
        }
    }

    fn handle_vt_ioctl(&self, cmd: u32, arg: usize, vmar: &Arc<Vmar>) -> Result<usize> {
        match cmd {
            ioctl::VT_OPENQRY => {
                let free_vt: i32 = 1;
                UserPtr::new(arg).write_bytes(vmar, &free_vt.to_ne_bytes())?;
                Ok(0)
            }
            ioctl::VT_GETSTATE => {
                let state: [u16; 3] = [1, 0, 1];
                let mut bytes = [0u8; 6];
                bytes[0..2].copy_from_slice(&state[0].to_ne_bytes());
                bytes[2..4].copy_from_slice(&state[1].to_ne_bytes());
                bytes[4..6].copy_from_slice(&state[2].to_ne_bytes());
                UserPtr::new(arg).write_bytes(vmar, &bytes)?;
                Ok(0)
            }
            ioctl::VT_GETMODE => {
                let bytes = [0u8; 8];
                UserPtr::new(arg).write_bytes(vmar, &bytes)?;
                Ok(0)
            }
            ioctl::VT_SETMODE | ioctl::VT_ACTIVATE | ioctl::VT_WAITACTIVE | ioctl::VT_DISALLOCATE => {
                Ok(0)
            }
            ioctl::TIOCLINUX => Ok(0),
            _ => crate::return_errno!(ENOTTY, "unsupported VT ioctl"),
        }
    }

    /// Internal ioctl handler resolving against a supplied VMAR.
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
                drain_input();
                let count = self.ldisc.lock().read_available() as i32;
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

    /// Standard ioctl dispatching to current process address space.
    pub fn ioctl(&self, cmd: u32, arg: usize) -> Result<usize> {
        let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
        let vmar = proc.vmspace();
        self.ioctl_with_vmar(cmd, arg, &vmar, Some(&proc))
    }
}

impl CharacterDevice for Tty {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn read(&self, buf: &mut [u8]) -> Result<usize> {
        self.read(buf)
    }

    fn write(&self, buf: &[u8]) -> Result<usize> {
        self.write(buf)
    }

    fn ioctl(&self, cmd: u32, arg: usize) -> Result<usize> {
        self.ioctl(cmd, arg)
    }

    fn poll(&self) -> Result<u32> {
        self.poll()
    }
}

/// FileOps implementation wrapping an Arc<Tty> (matching Zinnia TtyFileOps).
pub struct TtyFileOps {
    pub tty: Arc<Tty>,
}

impl FileOps for TtyFileOps {
    fn read(&self, _offset: u64, buf: &mut [u8]) -> Result<usize> {
        self.tty.read(buf)
    }

    fn write(&self, _offset: u64, buf: &[u8]) -> Result<usize> {
        self.tty.write(buf)
    }

    fn ioctl(&self, cmd: u32, arg: usize) -> Result<usize> {
        self.tty.ioctl(cmd, arg)
    }

    fn poll(&self) -> PollEvents {
        match self.tty.poll() {
            Ok(mask) => PollEvents::from_bits_truncate(mask as u16),
            Err(_) => PollEvents::empty(),
        }
    }
}

// ----------------------------------------------------------------------------
// Console Device & File Integration
// ----------------------------------------------------------------------------

pub struct Console;

static ACTIVE_CONSOLE: SpinLock<Option<Arc<Tty>>, LocalIrqDisabled> = SpinLock::new(None);

impl Console {
    pub fn get_active() -> Result<Arc<Tty>> {
        ACTIVE_CONSOLE.lock().clone().ok_or(Errno::ENXIO)
    }

    pub fn set_active(tty: Arc<Tty>) {
        *ACTIVE_CONSOLE.lock() = Some(tty);
    }
}

impl CharacterDevice for Console {
    fn name(&self) -> String {
        String::from("console")
    }

    fn read(&self, buf: &mut [u8]) -> Result<usize> {
        Console::get_active()?.read(buf)
    }

    fn write(&self, buf: &[u8]) -> Result<usize> {
        if let Ok(tty) = Console::get_active() {
            tty.write(buf)
        } else {
            for &b in buf {
                ostd::early_print!("{}", b as char);
            }
            Ok(buf.len())
        }
    }

    fn ioctl(&self, cmd: u32, arg: usize) -> Result<usize> {
        Console::get_active()?.ioctl(cmd, arg)
    }

    fn poll(&self) -> Result<u32> {
        Console::get_active()?.poll()
    }
}

pub struct ConsoleFileOps;

impl FileOps for ConsoleFileOps {
    fn read(&self, _offset: u64, buf: &mut [u8]) -> Result<usize> {
        if let Ok(tty) = Console::get_active() {
            tty.read(buf)
        } else {
            Ok(0)
        }
    }

    fn write(&self, _offset: u64, buf: &[u8]) -> Result<usize> {
        if let Ok(tty) = Console::get_active() {
            tty.write(buf)
        } else {
            for &b in buf {
                ostd::early_print!("{}", b as char);
            }
            Ok(buf.len())
        }
    }

    fn ioctl(&self, cmd: u32, arg: usize) -> Result<usize> {
        Console::get_active()?.ioctl(cmd, arg)
    }

    fn poll(&self) -> PollEvents {
        if let Ok(tty) = Console::get_active() {
            match tty.poll() {
                Ok(mask) => PollEvents::from_bits_truncate(mask as u16),
                Err(_) => PollEvents::empty(),
            }
        } else {
            PollEvents::IN | PollEvents::OUT
        }
    }
}

/// Creates a standard open file description for /dev/console.
pub fn create_console_file(flags: OpenFlags) -> Arc<File> {
    let inode = Arc::new(INode::new(
        Box::new(crate::fs::devfs::chr::GenericChrNodeOps::new(Arc::new(Console))),
        Mode::CHAR
            | Mode::RUSR
            | Mode::WUSR
            | Mode::RGRP
            | Mode::WGRP
            | Mode::ROTH
            | Mode::WOTH,
        2,
    ));
    Arc::new(File::new(Box::new(ConsoleFileOps), inode, flags, false))
}

pub fn get_tty(index: u32) -> Option<Arc<Tty>> {
    TTYS.lock().get(&index).cloned()
}

pub fn get_tty_by_name(name: &str) -> Option<Arc<Tty>> {
    TTYS.lock().values().find(|t| t.name == name).cloned()
}

/// Returns the primary system console device.
pub fn get_console() -> Option<Arc<Tty>> {
    ACTIVE_CONSOLE.lock().clone()
}

/// Bytes received from interrupt handlers, waiting to be fed to the line discipline.
static INPUT_QUEUE: LocklessQueue<1024> = LocklessQueue::new();

/// Serializes consumers of INPUT_QUEUE. Never taken from interrupt context.
static DRAIN_LOCK: Mutex<()> = Mutex::new(());

/// Queues an input byte. Lock-free; safe to call from interrupt handlers.
pub fn queue_input(ch: u8) {
    INPUT_QUEUE.push_overwrite(ch);
    if let Some(console) = get_console() {
        console.wake_readers();
    }
}

/// Feeds all queued input bytes to the console line discipline. Thread context only.
pub fn drain_input() {
    let Some(_guard) = DRAIN_LOCK.try_lock() else {
        return;
    };
    let Some(console) = get_console() else {
        return;
    };
    while let Some(b) = INPUT_QUEUE.pop() {
        console.input_byte(b);
    }
}

/// Flushes any pending input in INPUT_QUEUE.
pub fn flush_input_queue() {
    let Some(_guard) = DRAIN_LOCK.try_lock() else {
        return;
    };
    while INPUT_QUEUE.pop().is_some() {}
}

/// Dispatches an incoming character from the keyboard into the console TTY.
pub fn handle_keyboard_char(ch: u8) {
    if let Some(console) = get_console() {
        console.input_byte(ch);
    }
}

/// Initialize the TTY and PTY subsystem and register /dev/console, /dev/tty0, and /dev/ptmx.
pub fn init() -> Result<()> {
    let driver = Arc::new(FramebufferConsoleDriver);
    let console_tty = Tty::new(String::from("tty0"), driver);
    Console::set_active(Arc::clone(&console_tty));

    register_chrdev(Arc::new(Console));
    register_chrdev(Arc::clone(&console_tty) as Arc<dyn CharacterDevice>);

    pty::init()?;

    ostd::info!("TTY and PTY subsystem initialized successfully");
    Ok(())
}

crate::module!(
    "TTY Console Subsystem",
    "Ananta98",
    crate::modules::InitcallLevel::Device,
    init
);
