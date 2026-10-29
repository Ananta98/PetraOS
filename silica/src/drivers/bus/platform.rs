// SPDX-License-Identifier: GPL-2.0

//! Platform bus integration and driver bridge.
//!
//! Exposes Linux-style platform bus subsystem via `kpi::device::platform`.

pub use crate::kpi::device::platform::{
    platform_bus, platform_device_register, platform_driver_register, platform_driver_unregister,
    PlatformBus, PlatformBusRegistry, PlatformDevice, PlatformDriver, Resource, ResourceType,
};
