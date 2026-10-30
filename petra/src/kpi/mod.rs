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
pub use self::core::{atomic, mm, refcount, sync, task, time};
pub use self::device::pci;
pub use self::driver::{block, char};
pub use self::hw::{dma, io, irq};

// High-level convenient exports at kpi root
pub use self::core::{
    atomic64_t, atomic_t, change_bit, clear_bit, set_bit, test_and_change_bit, test_and_clear_bit,
    test_and_set_bit, test_bit, Gfp, KCompletion, KMutex, KRef, KTimer, KWeak, RootSpinLock,
    SpinLock, arm, jiffies, kcalloc, kmalloc, ktime_get_ns, kzalloc, mdelay, msleep,
    schedule_delayed_work_fn, schedule_work, schedule_work_fn, udelay, usleep_range, vfree,
    vmalloc,
};
pub use self::device::{
    Device, DeviceBusRegistry, Driver, PciDevice, PciDeviceId, PciDriver, PlatformBus,
    PlatformDevice, PlatformDriver, Resource, ResourceType, pci_register_driver,
    pci_unregister_driver, platform_bus, platform_device_register, platform_driver_register,
    platform_driver_unregister,
};
pub use self::driver::{
    BlockDeviceOperations, CDev, FileOperations, GenDisk, add_disk, alloc_chrdev_region, cdev_add,
    cdev_del, del_gendisk,
};
pub use self::hw::{
    dma_addr_t, dma_alloc_coherent, dma_free_coherent, free_irq, ioremap, ioremap_shared,
    request_irq, request_threaded_irq, DmaBuffer, IoRegion, IrqFlags, IrqHandlerFn, IrqReturn,
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
