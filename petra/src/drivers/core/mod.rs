// SPDX-License-Identifier: MPL-2.0
//
// Ported from Asterinas `aster-device` (`kernel/core/comps/device/src`):
// https://github.com/asterinas/asterinas/tree/e36b5b3b759e616febf2c8f0e88d8cb2c103f266/kernel/core/comps/device/src
// Copyright (c) Asterinas contributors. Adapted for PetraOS.

//! The device model: devices, buses, classes, and drivers, and the sysfs view built from them.
//!
//! Ported from Asterinas `aster-device` (`kernel/core/comps/device/src`, commit `e36b5b3b`).
//! This subsystem manages devices and their parent-child relationships,
//! matches devices with drivers through buses ([`bus`]),
//! presents them to user space through classes ([`class`]),
//! manages shared device metadata and transactional registration ([`common`]),
//! and queues `/dev` node requests through configurable [`hooks`].
//!
//! Every device is registered to or unregistered from the device model
//! through [`add_device`] and [`remove_device`].

pub mod bus;
pub mod class;
pub mod common;
pub mod hooks;
#[cfg(ktest)]
mod test;

pub use self::bus::{Bus, BusDevice, BusDeviceBuilder, BusHandle, Driver, DriverHandle};
pub use self::class::{Class, ClassDevice, ClassDeviceBuilder, ClassHandle, ClassObserver};
pub use self::common::registration::{
    add as add_device, register_legacy_virtual_node, remove as remove_device,
};
pub use self::common::registry::{dev_index_names, dev_index_node};
pub use self::common::{
    AnyDevice, Attr, AttrMeta, AttrSet, BareDevice, DevKind, DevNode, DevNum, DeviceBuilder,
    DeviceId, DeviceType, Error, Result, ShowFn, StoreFn, Subsystem, SubsystemKind, SysPerms,
    SysStr,
};

/// Initializes the device model roots and sysfs registry (idempotent).
pub fn init() {
    common::registry::init();
}
