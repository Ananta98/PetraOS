// SPDX-License-Identifier: GPL-2.0

//! Linux-shaped Kernel Programming Interface for safe-Rust kernel components.
//!
//! `kpi` mirrors the semantics of common Linux kernel APIs (locks, completions,
//! workqueues, timers, reference counting, zeroing allocation) on top of OSTD
//! primitives, so kernel services and drivers written with Linux-style designs
//! can be expressed in fully safe Rust.
//!
//! # Safety contract
//!
//! Every type here is sound by construction: this layer contains no `unsafe`
//! and relies only on OSTD's sound safe APIs. The backbone intentionally does
//! *not* claim byte-for-byte Linux API compatibility; semantics that cannot be
//! kept sound are narrowed instead, and every such narrowing is documented on
//! the type: intrusive containers (`list_head`) are replaced by `alloc` data
//! structures, `kref` is [`alloc::sync::Arc`], and allocation never blocks or
//! underallocates silently.
//!
//! # Initialization
//!
//! [`init`] must be called once from `kernel_init` (after the scheduler is up)
//! before any subsystem schedules work or arms timers.

pub mod device;
pub mod mm;
pub mod refcount;
pub mod sync;
pub mod task;
pub mod time;

pub use device::{
    platform_bus, platform_device_register, platform_driver_register, platform_driver_unregister,
    Device, DeviceBusRegistry, Driver, PlatformBus, PlatformDevice, PlatformDriver, Resource,
    ResourceType,
};
pub use mm::{kzalloc, Gfp};
pub use refcount::{KRef, KWeak};
pub use sync::{KCompletion, KMutex, RootSpinLock, SpinLock};
pub use task::{schedule_delayed_work_fn, schedule_work, schedule_work_fn};
pub use time::{arm, jiffies, KTimer};


use crate::uapi::errno::Result;

/// Brings up the workqueue and the timer wheel.
pub fn init() -> Result<()> {
    task::init()?;
    time::init();
    Ok(())
}
