// SPDX-License-Identifier: GPL-2.0

//! Linux `struct platform_device` and `struct platform_driver` implementation.
//!
//! Mirrors Linux platform bus subsystem for memory-mapped and non-discoverable devices.
//! - Resources (`IORESOURCE_MEM`, `IORESOURCE_IRQ`).
//! - Platform device and driver registration.
//! - Resource retrieval helpers (`platform_get_resource`, `platform_get_irq`).

use alloc::{
    string::String,
    sync::Arc,
    vec::Vec,
};
use spin::{Once, RwLock};

use super::base::{BusType, Device, Driver};
use crate::uapi::errno::{Errno, Result};

/// Resource type flags mirroring Linux `IORESOURCE_*`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceType {
    /// MMIO memory region (`IORESOURCE_MEM`).
    Mem,
    /// Interrupt line (`IORESOURCE_IRQ`).
    Irq,
    /// I/O port (`IORESOURCE_IO`).
    Io,
}

/// Linux `struct resource`: describes a physical range (MMIO, IRQ, etc.).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resource {
    /// Start of the resource range.
    pub start: usize,
    /// End of the resource range (inclusive).
    pub end: usize,
    /// Name of the resource.
    pub name: String,
    /// Type of the resource.
    pub resource_type: ResourceType,
}

impl Resource {
    /// Creates a new MMIO memory resource.
    pub fn mem(start: usize, size: usize, name: impl Into<String>) -> Self {
        Self {
            start,
            end: if size > 0 { start + size - 1 } else { start },
            name: name.into(),
            resource_type: ResourceType::Mem,
        }
    }

    /// Creates a new IRQ resource.
    pub fn irq(irq_num: usize, name: impl Into<String>) -> Self {
        Self {
            start: irq_num,
            end: irq_num,
            name: name.into(),
            resource_type: ResourceType::Irq,
        }
    }

    /// Returns the length of the resource range.
    pub fn len(&self) -> usize {
        if self.end >= self.start {
            self.end - self.start + 1
        } else {
            0
        }
    }

    /// Checks if the range is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Linux `struct platform_device`: a device residing on the pseudo platform bus.
pub struct PlatformDevice {
    /// Inner core device representation.
    core_device: Arc<Device>,
    /// Unique platform device ID (e.g. `0` for `serial8250.0`, `-1` if singleton).
    id: i32,
    /// Hardware/firmware resources assigned to this platform device.
    resources: Vec<Resource>,
}

impl PlatformDevice {
    /// Creates a new platform device.
    pub fn new(name: impl Into<String>, id: i32, resources: Vec<Resource>) -> Self {
        let name_str = name.into();
        let full_name = if id >= 0 {
            alloc::format!("{}.{}", name_str, id)
        } else {
            name_str
        };

        Self {
            core_device: Arc::new(Device::new(full_name)),
            id,
            resources,
        }
    }

    /// Returns the core device object.
    pub fn device(&self) -> &Arc<Device> {
        &self.core_device
    }

    /// Returns the device name.
    pub fn name(&self) -> &str {
        self.core_device.name()
    }

    /// Returns device platform ID.
    pub fn id(&self) -> i32 {
        self.id
    }

    /// Returns all resources attached to this device.
    pub fn resources(&self) -> &[Resource] {
        &self.resources
    }

    /// Helper mirroring `platform_get_resource()`.
    pub fn get_resource(&self, res_type: ResourceType, index: usize) -> Option<&Resource> {
        self.resources
            .iter()
            .filter(|r| r.resource_type == res_type)
            .nth(index)
    }

    /// Helper mirroring `platform_get_irq()`.
    pub fn get_irq(&self, index: usize) -> Result<usize> {
        self.get_resource(ResourceType::Irq, index)
            .map(|r| r.start)
            .ok_or(Errno::ENXIO)
    }
}

/// Linux `struct platform_driver`: driver for platform devices.
pub trait PlatformDriver: Send + Sync + 'static {
    /// Driver name.
    fn name(&self) -> &str;

    /// Invoked when a platform device matches this driver (`probe`).
    fn probe(&self, pdev: &Arc<PlatformDevice>) -> Result<()>;

    /// Invoked when the device or driver is removed (`remove`).
    fn remove(&self, pdev: &Arc<PlatformDevice>) -> Result<()>;
}

/// The platform bus type singleton.
pub struct PlatformBus;

impl BusType for PlatformBus {
    fn name(&self) -> &str {
        "platform"
    }

    fn match_device(&self, device: &Device, driver: &dyn Driver) -> bool {
        // Platform matching convention: match by name or prefix before dot
        let dev_name = device.name();
        let base_name = dev_name.split('.').next().unwrap_or(dev_name);
        base_name == driver.name() || dev_name == driver.name()
    }
}

/// Global platform bus registry state.
static PLATFORM_BUS: Once<Arc<PlatformBusRegistry>> = Once::new();

/// Platform bus registry managing registered platform devices and drivers.
pub struct PlatformBusRegistry {
    devices: RwLock<Vec<Arc<PlatformDevice>>>,
    drivers: RwLock<Vec<Arc<dyn PlatformDriver>>>,
}

impl PlatformBusRegistry {
    /// Creates a new platform registry.
    pub fn new() -> Self {
        Self {
            devices: RwLock::new(Vec::new()),
            drivers: RwLock::new(Vec::new()),
        }
    }

    /// Registers a platform device. If matching driver exists, triggers probe.
    pub fn register_device(&self, pdev: Arc<PlatformDevice>) -> Result<()> {
        let mut dev_guard = self.devices.write();
        if dev_guard.iter().any(|d| d.name() == pdev.name()) {
            crate::return_errno!(EEXIST, "platform device already registered");
        }

        let drv_guard = self.drivers.read();
        for driver in drv_guard.iter() {
            let base_name = pdev.name().split('.').next().unwrap_or(pdev.name());
            if driver.name() == base_name || driver.name() == pdev.name() {
                if let Ok(()) = driver.probe(&pdev) {
                    break;
                }
            }
        }

        dev_guard.push(pdev);
        Ok(())
    }

    /// Registers a platform driver. Attempts to probe all matching devices.
    pub fn register_driver(&self, driver: Arc<dyn PlatformDriver>) -> Result<()> {
        let mut drv_guard = self.drivers.write();
        if drv_guard.iter().any(|d| d.name() == driver.name()) {
            crate::return_errno!(EEXIST, "platform driver already registered");
        }

        let dev_guard = self.devices.read();
        for pdev in dev_guard.iter() {
            let base_name = pdev.name().split('.').next().unwrap_or(pdev.name());
            if driver.name() == base_name || driver.name() == pdev.name() {
                let _ = driver.probe(pdev);
            }
        }

        drv_guard.push(driver);
        Ok(())
    }

    /// Unregisters a platform driver by name.
    pub fn unregister_driver(&self, driver_name: &str) -> Result<()> {
        let mut drv_guard = self.drivers.write();
        let index = drv_guard
            .iter()
            .position(|d| d.name() == driver_name)
            .ok_or(Errno::ENODEV)?;

        let driver = drv_guard.remove(index);
        let dev_guard = self.devices.read();
        for pdev in dev_guard.iter() {
            let base_name = pdev.name().split('.').next().unwrap_or(pdev.name());
            if driver.name() == base_name || driver.name() == pdev.name() {
                let _ = driver.remove(pdev);
            }
        }

        Ok(())
    }
}

/// Returns the global platform bus registry.
pub fn platform_bus() -> &'static Arc<PlatformBusRegistry> {
    PLATFORM_BUS.call_once(|| Arc::new(PlatformBusRegistry::new()))
}

/// Registers a platform device into the global platform bus.
pub fn platform_device_register(pdev: Arc<PlatformDevice>) -> Result<()> {
    platform_bus().register_device(pdev)
}

/// Registers a platform driver into the global platform bus.
pub fn platform_driver_register(driver: Arc<dyn PlatformDriver>) -> Result<()> {
    platform_bus().register_driver(driver)
}

/// Unregisters a platform driver from the global platform bus.
pub fn platform_driver_unregister(driver_name: &str) -> Result<()> {
    platform_bus().unregister_driver(driver_name)
}
