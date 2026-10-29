// SPDX-License-Identifier: GPL-2.0

//! Linux `struct device` and `struct device_driver` abstractions.
//!
//! Provides core driver-model primitives matching Linux device model semantics:
//! - [`Device`]: Core device object with identity, bus reference, parent, and private driver data (`dev_get_drvdata` / `dev_set_drvdata`).
//! - [`Driver`]: Linux-like driver interface with `probe` and `remove`.
//! - [`BusType`]: Trait modeling `struct bus_type` for matching devices and drivers.

use alloc::{
    boxed::Box,
    string::String,
    sync::Arc,
    vec::Vec,
};
use core::any::Any;
use spin::{Mutex, RwLock};

use crate::uapi::errno::{Errno, Result};

/// Represents an abstract bus type in Linux driver model (`struct bus_type`).
pub trait BusType: Send + Sync + 'static {
    /// Name of the bus (e.g. "platform", "pci").
    fn name(&self) -> &str;

    /// Checks if a driver can handle the given device.
    fn match_device(&self, device: &Device, driver: &dyn Driver) -> bool;
}

/// Linux `struct device`: core representation of a hardware or virtual device.
pub struct Device {
    /// Device name (e.g., "serial8250.0", "dummy_platform").
    name: String,
    /// Parent device if hierarchically organized.
    parent: Option<Arc<Device>>,
    /// Associated bus type.
    bus: Option<Arc<dyn BusType>>,
    /// Driver private data (`dev_set_drvdata` / `dev_get_drvdata`).
    driver_data: Mutex<Option<Box<dyn Any + Send + Sync>>>,
}

impl Device {
    /// Creates a new device instance.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            parent: None,
            bus: None,
            driver_data: Mutex::new(None),
        }
    }

    /// Creates a device with specified parent and bus type.
    pub fn new_with_bus(
        name: impl Into<String>,
        parent: Option<Arc<Device>>,
        bus: Option<Arc<dyn BusType>>,
    ) -> Self {
        Self {
            name: name.into(),
            parent,
            bus,
            driver_data: Mutex::new(None),
        }
    }

    /// Returns the device name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the parent device if any.
    pub fn parent(&self) -> Option<&Arc<Device>> {
        self.parent.as_ref()
    }

    /// Returns the bus type if assigned.
    pub fn bus(&self) -> Option<&Arc<dyn BusType>> {
        self.bus.as_ref()
    }

    /// Sets driver private data, mirroring `dev_set_drvdata()`.
    pub fn set_drvdata<T: Send + Sync + 'static>(&self, data: T) {
        *self.driver_data.lock() = Some(Box::new(data));
    }

    /// Takes and returns driver private data if set.
    pub fn take_drvdata<T: Send + Sync + 'static>(&self) -> Option<Box<T>> {
        let mut guard = self.driver_data.lock();
        let boxed_any = guard.take()?;
        match boxed_any.downcast::<T>() {
            Ok(concrete) => Some(concrete),
            Err(original) => {
                // Restore if type mismatch
                *guard = Some(original);
                None
            }
        }
    }

    /// Borrows driver private data, mirroring `dev_get_drvdata()`.
    pub fn with_drvdata<T: Send + Sync + 'static, R>(&self, f: impl FnOnce(&T) -> R) -> Option<R> {
        let guard = self.driver_data.lock();
        let boxed_any = guard.as_ref()?;
        let concrete = boxed_any.downcast_ref::<T>()?;
        Some(f(concrete))
    }
}

/// Linux `struct device_driver`: operations for probing and binding devices.
pub trait Driver: Send + Sync + 'static {
    /// Name of the driver.
    fn name(&self) -> &str;

    /// Invoked to probe and initialize a matched device (`probe`).
    fn probe(&self, device: &Arc<Device>) -> Result<()>;

    /// Invoked when a device is unbound or the driver is removed (`remove`).
    fn remove(&self, device: &Arc<Device>) -> Result<()>;
}

/// A registry managing devices and drivers for a specific bus (`BusHandle`).
pub struct DeviceBusRegistry {
    bus: Arc<dyn BusType>,
    devices: RwLock<Vec<Arc<Device>>>,
    drivers: RwLock<Vec<Arc<dyn Driver>>>,
}

impl DeviceBusRegistry {
    /// Creates a new device registry for the given bus.
    pub fn new(bus: Arc<dyn BusType>) -> Self {
        Self {
            bus,
            devices: RwLock::new(Vec::new()),
            drivers: RwLock::new(Vec::new()),
        }
    }

    /// Registers a device and attempts to match with registered drivers.
    pub fn register_device(&self, device: Arc<Device>) -> Result<()> {
        let mut devices_guard = self.devices.write();
        if devices_guard.iter().any(|d| d.name() == device.name()) {
            crate::return_errno!(EEXIST, "device name already registered");
        }

        // Try probing with existing drivers
        let drivers_guard = self.drivers.read();
        for driver in drivers_guard.iter() {
            if self.bus.match_device(&device, driver.as_ref()) {
                if let Ok(()) = driver.probe(&device) {
                    break;
                }
            }
        }

        devices_guard.push(device);
        Ok(())
    }

    /// Registers a driver and attempts to probe all unbound matching devices.
    pub fn register_driver(&self, driver: Arc<dyn Driver>) -> Result<()> {
        let mut drivers_guard = self.drivers.write();
        if drivers_guard.iter().any(|d| d.name() == driver.name()) {
            crate::return_errno!(EEXIST, "driver name already registered");
        }

        let devices_guard = self.devices.read();
        for device in devices_guard.iter() {
            if self.bus.match_device(device, driver.as_ref()) {
                let _ = driver.probe(device);
            }
        }

        drivers_guard.push(driver);
        Ok(())
    }

    /// Unregisters a driver and removes bindings for matching devices.
    pub fn unregister_driver(&self, driver_name: &str) -> Result<()> {
        let mut drivers_guard = self.drivers.write();
        let index = drivers_guard
            .iter()
            .position(|d| d.name() == driver_name)
            .ok_or(Errno::ENODEV)?;

        let driver = drivers_guard.remove(index);
        let devices_guard = self.devices.read();
        for device in devices_guard.iter() {
            if self.bus.match_device(device, driver.as_ref()) {
                let _ = driver.remove(device);
            }
        }

        Ok(())
    }
}
