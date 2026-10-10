// SPDX-License-Identifier: GPL-2.0

//! Platform services system calls.

pub mod reboot;
pub mod random;

pub use reboot::sys_reboot;
pub use random::sys_getrandom;
