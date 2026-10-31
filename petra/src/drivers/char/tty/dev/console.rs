// SPDX-License-Identifier: GPL-2.0

//! Framebuffer Console Device and File Operations.

use alloc::{boxed::Box, string::String, sync::Arc};
use ostd::sync::{LocalIrqDisabled, SpinLock};

use crate::drivers::CharacterDevice;
use crate::fs::poll::PollEvents;
use crate::fs::vfs::{File, FileOps, INode, Mode, OpenFlags};
use crate::uapi::errno::{Errno, Result};
use crate::uapi::signal::Signal;
use crate::uapi::termios::winsize;

use crate::drivers::char::tty::core::{Tty, TtyDriver};
use crate::drivers::char::tty::vt;

/// Driver delivering TTY output directly to the Framebuffer Console.
pub struct FramebufferConsoleDriver;

impl TtyDriver for FramebufferConsoleDriver {
    fn write_output(&self, data: &[u8]) -> Result<()> {
        if let Ok(s) = core::str::from_utf8(data) {
            ostd::early_print!("{}", s);
        } else {
            for &b in data {
                ostd::early_print!("{}", b as char);
            }
        }
        vt::write_output(data);
        Ok(())
    }

    fn get_winsize(&self) -> winsize {
        vt::get_winsize()
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
