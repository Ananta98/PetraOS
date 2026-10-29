// SPDX-License-Identifier: GPL-2.0

//! TTY Core Structure and File Operations.

use alloc::{string::String, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

use ostd::sync::{WaitQueue, Waiter};

use crate::uapi::errno::{Errno, Result};
use crate::uapi::signal::Signal;
use crate::uapi::termios::{VMIN, VTIME, winsize};
use crate::drivers::CharacterDevice;
use crate::fs::poll::PollEvents;
use crate::fs::vfs::FileOps;
use crate::proc::thread::Thread;
use crate::proc::{Pid, pgrp_for_each};
use crate::vm::vmar::Vmar;

use super::input::drain_input;
use super::ioctl::handle_ioctl;
use super::ldisc::LineDiscipline;

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
        crate::fs::poll::notify();
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
        crate::fs::poll::notify();
    }

    /// Feeds a raw byte into the line discipline and writes any resulting echo to the driver.
    pub fn input_byte(&self, byte: u8) {
        let output = self.input_byte_internal(byte);
        if !output.is_empty() {
            for &b in &output {
                ostd::early_print!("{}", b as char);
            }
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

        let (canon, vmin, vtime) = {
            let ldisc = self.ldisc.lock();
            (
                ldisc.is_canon(),
                ldisc.cc(VMIN) as usize,
                ldisc.cc(VTIME) as usize,
            )
        };

        let (waiter, _) = Waiter::new_pair();
        let ktimer = if !canon && vtime > 0 {
            let waker = waiter.waker();
            Some(crate::kpi::time::arm(
                core::time::Duration::from_millis((vtime as u64) * 100),
                Some(alloc::boxed::Box::new(move || {
                    waker.wake_up();
                })),
            ))
        } else {
            None
        };
        let deadline_ns = if !canon && vtime > 0 {
            Some(crate::time::get_monotonic_ns().saturating_add((vtime as u64) * 100_000_000))
        } else {
            None
        };

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
                } else if vtime == 0 || deadline_ns.is_some_and(|dl| crate::time::get_monotonic_ns() >= dl) {
                    Some(0)
                } else {
                    None
                }
            } else if avail > 0 && (avail >= vmin.min(buf.len()) || deadline_ns.is_some_and(|dl| crate::time::get_monotonic_ns() >= dl)) {
                Some(ldisc.read_into(buf))
            } else if self.is_hung_up() {
                Some(ldisc.read_into(buf))
            } else {
                None
            };

            if ready.is_none() {
                self.wq.enqueue(waiter.waker());
            }
            ready
        };

        let cancel = || if Thread::should_interrupt() {
            Err(Errno::EINTR)
        } else {
            Ok(())
        };

        let res = waiter.wait_until_or_cancelled(cond, cancel);
        if let Some(t) = ktimer {
            t.cancel();
        }
        res
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

    /// Internal ioctl handler resolving against a supplied VMAR.
    pub fn ioctl_with_vmar(
        &self,
        cmd: u32,
        arg: usize,
        vmar: &Arc<Vmar>,
        proc_opt: Option<&Arc<crate::proc::Proc>>,
    ) -> Result<usize> {
        handle_ioctl(self, cmd, arg, vmar, proc_opt)
    }

    /// Standard ioctl dispatching to current process address space.
    pub fn ioctl(&self, cmd: u32, arg: usize) -> Result<usize> {
        let proc = Thread::proc_or_esrch()?;
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

/// FileOps implementation wrapping an Arc<Tty>.
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
