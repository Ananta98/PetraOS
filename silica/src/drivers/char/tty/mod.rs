// SPDX-License-Identifier: GPL-2.0

//! TTY Subsystem.
//!
//! Based on TTY subsystem design and POSIX/Linux TTY standards.
//! Integrates line discipline, termios, session job control, pseudo-terminals,
//! and hardware display drivers.

pub mod console;
pub mod core;
pub mod font;
pub mod input_queue;
pub mod ioctl;
pub mod kbd;
pub mod ldisc;
pub mod pty;
pub mod terminal;

pub use console::{
    Console, ConsoleFileOps, FramebufferConsoleDriver, create_console_file, get_console,
    on_proc_exit, update_console_winsize,
};
pub use core::{Tty, TtyDriver, TtyFileOps};
pub use input_queue::{drain_input, flush_input_queue, queue_input};

use alloc::string::String;
use alloc::sync::Arc;

use crate::drivers::{CharacterDevice, register_chrdev};
use crate::uapi::errno::Result;

/// Initialize the TTY and PTY subsystem and register /dev/console, /dev/tty0, and /dev/ptmx.
pub fn init() -> Result<()> {
    terminal::init();

    let driver = Arc::new(FramebufferConsoleDriver);
    let console_tty = Tty::new(String::from("tty0"), driver);
    Console::set_active(Arc::clone(&console_tty));

    register_chrdev(Arc::new(Console))?;
    register_chrdev(Arc::clone(&console_tty) as Arc<dyn CharacterDevice>)?;

    pty::init()?;

    // Console keyboard input arrives as EV_KEY events from the input core.
    kbd::init();

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
