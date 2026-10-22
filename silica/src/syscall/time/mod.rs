// SPDX-License-Identifier: GPL-2.0

//! Time and clock related system calls.

pub mod clock;
pub mod gettimeofday;
pub mod nanosleep;
pub mod timer;
pub mod times;

pub use clock::*;
pub use gettimeofday::*;
pub use nanosleep::*;
pub use timer::*;
pub use times::*;

// Re-export common time types and helper functions for backward compatibility
pub use crate::time::{Itimerval, Timespec, Timeval, get_boot_epoch_secs};
