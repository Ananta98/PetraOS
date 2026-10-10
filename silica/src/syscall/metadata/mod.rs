// SPDX-License-Identifier: GPL-2.0

//! System metadata and informational system calls.

pub mod sysinfo;
pub mod uname;

pub use sysinfo::*;
pub use uname::*;
