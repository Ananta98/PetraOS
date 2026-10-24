// SPDX-License-Identifier: GPL-2.0

//! TTY Subsystem.
//!
//! Based on TTY subsystem design and POSIX/Linux TTY standards.
//! Integrates line discipline, termios, session job control, pseudo-terminals,
//! and hardware display drivers.

pub mod core;
pub mod dev;
pub mod vt;

// Backwards-compatible module aliases for external references
pub use dev::console;
pub use dev::pty;
pub use vt::{font, kbd, terminal};

// Core types and functions re-exported at subsystem root
pub use core::{
    Tty, TtyDriver, TtyFileOps, drain_input, flush_input_queue, handle_ioctl, queue_input,
};
pub use dev::{
    Console, ConsoleFileOps, FramebufferConsoleDriver, create_console_file, get_console,
    update_console_winsize,
};

use alloc::string::String;
use alloc::sync::Arc;

use crate::drivers::{CharacterDevice, register_chrdev};
use crate::uapi::errno::Result;

/// Initialize the TTY and PTY subsystem and register /dev/console, /dev/tty0, and /dev/ptmx.
pub fn init() -> Result<()> {
    vt::terminal::init();

    let driver = Arc::new(FramebufferConsoleDriver);
    let console_tty = Tty::new(String::from("tty0"), driver);
    Console::set_active(Arc::clone(&console_tty));

    register_chrdev(Arc::new(Console))?;
    register_chrdev(Arc::clone(&console_tty) as Arc<dyn CharacterDevice>)?;

    pty::init()?;

    // Console keyboard input arrives as EV_KEY events from the input core.
    vt::kbd::init();

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
