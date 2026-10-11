// SPDX-License-Identifier: GPL-2.0

//! Platform services system calls.

pub mod reboot;
pub mod random;
pub mod modules;

pub use reboot::sys_reboot;
pub use random::sys_getrandom;
pub use modules::{sys_init_module, sys_finit_module, sys_delete_module};
