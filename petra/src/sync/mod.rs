// SPDX-License-Identifier: GPL-2.0

//! Synchronization Subsystem.
//!
//! Provides userspace mutex primitives (`futex(2)`), bitset matching,
//! realtime clock integration, and robust futex list tracking.

pub mod futex;
pub mod robust;

pub use futex::*;
pub use robust::*;
