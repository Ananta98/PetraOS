// SPDX-License-Identifier: GPL-2.0

//! Linux Driver Model Core for `kpi`.
//!
//! Exposes device hierarchy, drivers, and platform bus abstractions.

pub mod base;
pub mod platform;

pub use base::{BusType, Device, DeviceBusRegistry, Driver};
pub use platform::{
    platform_bus, platform_device_register, platform_driver_register, platform_driver_unregister,
    PlatformBus, PlatformBusRegistry, PlatformDevice, PlatformDriver, Resource, ResourceType,
};
