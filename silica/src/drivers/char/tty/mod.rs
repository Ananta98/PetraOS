// SPDX-License-Identifier: GPL-2.0

//! TTY Subsystem.
//!
//! Based on TTY subsystem design and POSIX/Linux TTY standards.
//! Integrates line discipline, termios, session job control, pseudo-terminals,
//! and hardware display drivers.

pub mod font;
pub mod ldisc;
pub mod pty;
pub mod terminal;

use alloc::{boxed::Box, string::String, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

use ostd::sync::{LocalIrqDisabled, SpinLock, WaitQueue, Waiter};

use crate::api::errno::{Errno, Result};
use crate::api::ioctl;
use crate::api::signal::Signal;
use crate::api::termios::{Termios, VMIN, VTIME, winsize};
use crate::drivers::{CharacterDevice, register_chrdev};
use crate::fs::poll::PollEvents;
use crate::fs::vfs::{File, FileOps, INode, Mode, OpenFlags};
use crate::proc::thread::{Thread, ThreadFlags};
use crate::proc::{PID_INIT, Pid, pgrp_for_each};
use crate::utils::ring_buffer::RingBuffer;
use crate::utils::userspace::user_ptr::UserPtr;
use crate::vm::vmar::Vmar;

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

/// High-level TTY instance managing line discipline, job control, and hardware driver.
pub struct Tty {
    pub name: String,
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
    /// Constructs a new TTY backed by the specified driver.
    pub fn new(name: String, driver: Arc<dyn TtyDriver>) -> Arc<Self> {
        let ws = driver.get_winsize();
        Arc::new(Self {
            name,
            ldisc: Mutex::new(LineDiscipline::new()),
            driver,
            winsize: Mutex::new(ws),
            winsize_user_set: AtomicBool::new(false),
            foreground_pgrp: Mutex::new(None),
            session: Mutex::new(None),
            wq: WaitQueue::new(),
            hangup: AtomicBool::new(false),
        })
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

    /// Sends a signal to the foreground process group.
    pub fn signal_foreground(&self, sig: Signal) {
        let target_pgrp = *self.foreground_pgrp.lock();
        let pgrp_opt = target_pgrp.or_else(|| Thread::current_proc().map(|p| p.pgid()));
        if let Some(pgrp) = pgrp_opt {
            pgrp_for_each(pgrp, |proc| {
                proc.post_signal(sig);
            });
        }
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
    ///
    /// A blocking canonical read stays interruptible: a signal delivered to the
    /// calling thread wakes it and the read returns `EINTR`, so `Ctrl+C` can
    /// break out of a terminal read at the shell prompt.
    pub fn read(&self, buf: &mut [u8]) -> Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        let (canon, vmin, vtime) = {
            let ldisc = self.ldisc.lock();
            (
                ldisc.is_canon(),
                ldisc.cc(VMIN) as usize,
                ldisc.cc(VTIME) as usize,
            )
        };

        let (waiter, _) = Waiter::new_pair();

        let cond = || -> Option<usize> {
            drain_input();
            let mut ldisc = self.ldisc.lock();
            let avail = ldisc.read_available();

            let ready = if canon {
                if avail > 0 {
                    Some(ldisc.read_into(buf))
                } else if ldisc.eof_received {
                    ldisc.eof_received = false;
                    Some(0)
                } else if self.is_hung_up() {
                    Some(0)
                } else {
                    None
                }
            } else if vmin == 0 {
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
            };

            // Publish the waker while the line discipline is still locked: a
            // producer fills the buffer under this same lock and wakes only
            // after releasing it, so enqueueing here cannot miss a wakeup.
            if ready.is_none() {
                self.wq.enqueue(waiter.waker());
            }
            ready
        };

        let cancel = || {
            if let Some(td) = Thread::current()
                && td.inner.lock().flags.contains(ThreadFlags::TDF_ASTPENDING)
            {
                return Err(Errno::EINTR);
            }
            Ok(())
        };

        waiter.wait_until_or_cancelled(cond, cancel)
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
                    if !self.winsize_user_set.load(Ordering::Acquire)
                        || ws.ws_col == 0
                        || ws.ws_row == 0
                    {
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
                    .unwrap_or(PID_INIT);
                let bytes = pgrp.as_u32().to_ne_bytes();
                UserPtr::new(arg).write_bytes(vmar, &bytes)?;
                Ok(0)
            }
            ioctl::TIOCSCTTY => {
                let proc = proc_opt.ok_or(Errno::EPERM)?;
                *self.session.lock() = Some(proc.pid);
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
                    .or_else(|| proc_opt.map(|p| p.pid))
                    .unwrap_or(PID_INIT);
                let bytes = sid.as_u32().to_ne_bytes();
                UserPtr::new(arg).write_bytes(vmar, &bytes)?;
                Ok(0)
            }
            _ => crate::return_errno!(ENOTTY, "unsupported job control ioctl"),
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

/// Driver delivering TTY output directly to the Framebuffer Console.
pub struct FramebufferConsoleDriver;

impl TtyDriver for FramebufferConsoleDriver {
    fn write_output(&self, data: &[u8]) -> Result<()> {
        terminal::write_output(data);
        Ok(())
    }

    fn get_winsize(&self) -> winsize {
        terminal::get_winsize()
    }
}

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
        Box::new(crate::fs::devfs::chr::GenericChrNodeOps::new(Arc::new(
            Console,
        ))),
        Mode::CHAR | Mode::RUSR | Mode::WUSR | Mode::RGRP | Mode::WGRP | Mode::ROTH | Mode::WOTH,
        2,
    ));
    Arc::new(File::new(Box::new(ConsoleFileOps), inode, flags, false))
}

/// Returns the primary system console device.
pub fn get_console() -> Option<Arc<Tty>> {
    ACTIVE_CONSOLE.lock().clone()
}

/// Updates the console winsize to match the underlying hardware driver dimensions.
pub fn update_console_winsize() {
    if let Some(console) = get_console() {
        let ws = console.driver.get_winsize();
        let changed = {
            let mut guard = console.winsize.lock();
            let old = *guard;
            *guard = ws;
            old.ws_row != ws.ws_row || old.ws_col != ws.ws_col
        };
        if changed {
            console.signal_foreground(Signal::SIGWINCH);
        }
    }
}

/// Bytes received from interrupt handlers, waiting to be fed to the line discipline.
static INPUT_QUEUE: RingBuffer<1024> = RingBuffer::new();

/// Serializes consumers of INPUT_QUEUE. Never taken from interrupt context.
static DRAIN_LOCK: Mutex<()> = Mutex::new(());

/// Set when `INPUT_QUEUE` holds bytes the input daemon has not drained yet.
static INPUT_PENDING: AtomicBool = AtomicBool::new(false);

/// Parks the input daemon until a keystroke is queued.
static INPUT_WAIT_QUEUE: WaitQueue = WaitQueue::new();

/// Queues an input byte. Lock-free; safe to call from interrupt handlers.
pub fn queue_input(ch: u8) {
    INPUT_QUEUE.push_overwrite(ch);
    INPUT_PENDING.store(true, Ordering::Release);
    INPUT_WAIT_QUEUE.wake_one();
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

/// Feeds queued keystrokes to the line discipline in thread context.
///
/// The line discipline cannot run from the keyboard interrupt: it takes the TTY
/// and process locks, which are not interrupt-safe. Running it here gives queued
/// bytes an execution context even when no process is reading the console, so a
/// control character such as Ctrl+C still signals the foreground process group
/// while a command that does not read stdin (e.g. `sleep`) is running.
fn keyboard_input_daemon() {
    loop {
        // `drain_input` returns early when another thread holds the drain lock,
        // so retry until the queue is observed empty rather than stranding input.
        while !INPUT_QUEUE.is_empty() {
            drain_input();
            if !INPUT_QUEUE.is_empty() {
                ostd::task::Task::yield_now();
            }
        }

        INPUT_WAIT_QUEUE.wait_until(|| INPUT_PENDING.swap(false, Ordering::Acquire).then_some(()));
    }
}

fn spawn_keyboard_input_daemon() -> Result<()> {
    crate::proc::kthread::kproc_create("tty_input", keyboard_input_daemon, 0)?;
    Ok(())
}

crate::module!(
    "TTY Input Daemon",
    "Ananta98",
    crate::modules::InitcallLevel::Late,
    spawn_keyboard_input_daemon
);

/// Called when a process terminates.
/// If the process left the active console TTY in raw mode or with disabled ONLCR,
/// restores canonical termios, exits alternate screen, and unhides cursor,
/// ensuring the parent shell or next command is not left in a staircase/corrupted state.
pub fn on_proc_exit(proc: &crate::proc::Proc) {
    if let Ok(tty) = Console::get_active() {
        let is_fg = tty
            .foreground_pgrp
            .lock()
            .map_or(false, |pgid| pgid == proc.pgid() || pgid == proc.pid);
        let is_child_of_init = proc.ppid() == Some(PID_INIT);
        let is_session_member = proc.sid() == PID_INIT;

        if is_fg || is_child_of_init || is_session_member {
            let mut ldisc = tty.ldisc.lock();
            let needs_reset = !ldisc.is_canon() || !ldisc.is_onlcr() || !ldisc.is_echo();
            if needs_reset {
                ldisc.termios = Termios::default();
                ldisc.read_buf.clear();
                ldisc.canon_buf.clear();
                flush_input_queue();
                drop(ldisc);

                // Exit alternate screen, reset text attributes, make cursor visible, and set steady block cursor
                terminal::write_output(b"\x1b[?1049l\x1b[0m\x1b[?25h\x1b[2 q");
            }
            if is_fg {
                *tty.foreground_pgrp.lock() = Some(PID_INIT);
            }
        }
    }
}

/// Initialize the TTY and PTY subsystem and register /dev/console, /dev/tty0, and /dev/ptmx.
pub fn init() -> Result<()> {
    terminal::init();

    let driver = Arc::new(FramebufferConsoleDriver);
    let console_tty = Tty::new(String::from("tty0"), driver);
    Console::set_active(Arc::clone(&console_tty));

    register_chrdev(Arc::new(Console));
    register_chrdev(Arc::clone(&console_tty) as Arc<dyn CharacterDevice>);

    pty::init()?;

    update_console_winsize();

    ostd::info!("TTY and PTY subsystem initialized successfully");
    Ok(())
}

crate::module!(
    "TTY Console Subsystem",
    "Ananta98",
    crate::modules::InitcallLevel::Device,
    init
);
