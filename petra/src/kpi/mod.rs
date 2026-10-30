// SPDX-License-Identifier: GPL-2.0

//! Linux-shaped Kernel Programming Interface (KPI) for safe-Rust kernel drivers and services.
//!
//! Organized into domain-specific modules:
//! - [`core`]: Concurrency primitives, deferred tasks (workqueues), soft timers, refcounts, and memory allocation.
//! - [`device`]: Linux device model (`struct device`, `struct device_driver`, `platform_device`, `platform_driver`).
//! - [`driver`]: Driver abstractions for character devices (`cdev`) and block devices (`gendisk`).
//! - [`hw`]: Hardware access via MMIO remapping (`ioremap`) and interrupt management (`request_irq`).
//! - [`module`]: Staged boot initcalls and module declaration macros.

pub mod core;
pub mod device;
pub mod driver;
pub mod hw;
pub mod module;
pub mod printk;
// Unsafe code is isolated strictly to the C-ABI compatibility bridge
#[allow(unsafe_code)]
pub mod shim;

// Re-export submodules under their canonical names for backwards compatibility
pub use self::core::{mm, refcount, sync, task, time};
pub use self::driver::{block, char};
pub use self::hw::{io, irq};

// High-level convenient exports at kpi root
pub use self::core::{
    Gfp, KCompletion, KMutex, KRef, KTimer, KWeak, RootSpinLock, SpinLock, arm, jiffies, kzalloc,
    schedule_delayed_work_fn, schedule_work, schedule_work_fn,
};
pub use self::device::{
    Device, DeviceBusRegistry, Driver, PlatformBus, PlatformDevice, PlatformDriver, Resource,
    ResourceType, platform_bus, platform_device_register, platform_driver_register,
    platform_driver_unregister,
};
pub use self::driver::{
    BlockDeviceOperations, CDev, FileOperations, GenDisk, add_disk, alloc_chrdev_region, cdev_add,
    cdev_del, del_gendisk,
};
pub use self::hw::{
    IoRegion, IrqFlags, IrqHandlerFn, IrqReturn, free_irq, ioremap, ioremap_shared, request_irq,
    request_threaded_irq,
};
pub use self::module::InitcallLevel;
pub use self::printk::{
    EACCES, EAGAIN, EBUSY, EEXIST, EFAULT, EINVAL, EIO, ENODEV, ENOENT, ENOMEM, ENOSPC, ENOTTY,
    ENXIO, EPERM,
};
pub use self::shim::{copy_from_user_slice, copy_to_user_slice};

use crate::uapi::errno::Result;

/// Brings up the core KPI workqueue and timer wheel.
pub fn init() -> Result<()> {
    core::task::init()?;
    core::time::init();
    Ok(())
}
