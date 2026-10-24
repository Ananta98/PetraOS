// SPDX-License-Identifier: GPL-2.0

//! TTY Device Subsystem.
//!
//! Provides concrete character device implementations:
//! - `/dev/console` and `/dev/tty0` via Framebuffer Console
//! - `/dev/ptmx` and `/dev/pts/*` pseudo-terminal subsystem

pub mod console;
pub mod pty;

pub use console::{
    Console, ConsoleFileOps, FramebufferConsoleDriver, create_console_file, get_console,
    update_console_winsize,
};
