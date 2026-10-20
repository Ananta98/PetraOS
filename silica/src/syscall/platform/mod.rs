// SPDX-License-Identifier: GPL-2.0

//! Platform services system calls.

pub mod modules;
pub mod random;
pub mod reboot;

pub use modules::{sys_delete_module, sys_finit_module, sys_init_module};
pub use random::sys_getrandom;
pub use reboot::sys_reboot;
