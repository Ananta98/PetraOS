// SPDX-License-Identifier: GPL-2.0

//! System V IPC system calls.

pub mod msg;
pub mod sem;
pub mod shm;

pub use msg::*;
pub use sem::*;
pub use shm::*;
