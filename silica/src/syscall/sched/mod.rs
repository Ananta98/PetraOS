// SPDX-License-Identifier: GPL-2.0

//! Scheduler system calls.

pub mod affinity;
pub mod attr;
pub mod getcpu;
pub mod membarrier;
pub mod param;
pub mod priority;
pub mod r#yield;

pub use affinity::*;
pub use attr::*;
pub use getcpu::*;
pub use membarrier::*;
pub use param::*;
pub use priority::*;
pub use r#yield::*;
