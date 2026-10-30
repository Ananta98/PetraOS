// SPDX-License-Identifier: GPL-2.0

//! System metadata and informational system calls.

pub mod hostname;
pub mod sysinfo;
pub mod uname;

pub use hostname::*;
pub use sysinfo::*;
pub use uname::*;
