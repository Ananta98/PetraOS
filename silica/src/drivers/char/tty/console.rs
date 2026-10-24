// SPDX-License-Identifier: GPL-2.0

//! Framebuffer Console Device and File Operations.

use alloc::{boxed::Box, string::String, sync::Arc};
use ostd::sync::{LocalIrqDisabled, SpinLock};

use crate::drivers::CharacterDevice;
use crate::fs::poll::PollEvents;
use crate::fs::vfs::{File, FileOps, INode, Mode, OpenFlags};
use crate::proc::PID_INIT;
use crate::uapi::errno::{Errno, Result};
use crate::uapi::signal::Signal;
use crate::uapi::termios::{Termios, winsize};

use super::core::{Tty, TtyDriver};
use super::input_queue::flush_input_queue;
use super::terminal;

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

/// Called when a process terminates.
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
