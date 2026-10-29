// SPDX-License-Identifier: GPL-2.0

//! TTY Core Subsystem.
//!
//! Provides the core TTY abstractions, line discipline, ioctl handlers,
//! and lockless input queue buffering.

pub mod input;
pub mod ioctl;
pub mod ldisc;
pub mod tty;

pub use input::{drain_input, flush_input_queue, queue_input, queue_input_bytes};
pub use ioctl::handle_ioctl;
pub use ldisc::LineDiscipline;
pub use tty::{Tty, TtyDriver, TtyFileOps};
