// SPDX-License-Identifier: GPL-2.0

//! File descriptors subsystem.
//!
//! Includes file descriptor table management, anonymous memory descriptors (memfd),
//! event notification counters (eventfd), timer triggers (timerfd), and synchronous
//! signal consumption descriptors (signalfd).

pub mod event_fd;
pub mod fd;
pub mod mem_fd;
pub mod signal_fd;
pub mod timer_fd;

pub use event_fd::*;
pub use fd::*;
pub use mem_fd::*;
pub use signal_fd::*;
pub use timer_fd::*;
