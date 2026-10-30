// SPDX-License-Identifier: GPL-2.0

//! Synchronization system calls (`futex`, `robust_list`).

pub mod futex;
pub mod robust;

pub use futex::*;
pub use robust::*;
