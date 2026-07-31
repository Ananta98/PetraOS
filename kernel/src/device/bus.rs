/// Bus abstractions and Bus-Driven Matching algorithms.
///
/// A bus represents a physical or logical interconnection (e.g. PCI, Platform, Virtual).
/// Bus-Driven Matching decouples device enumeration from specific driver implementations.
use crate::device::device::Device;
use crate::device::driver::Driver;
use alloc::sync::Arc;

/// Core interface implemented by all kernel hardware and virtual buses.
pub trait Bus: Send + Sync {
    /// Returns the unique name identifying this bus (e.g., "pci", "platform", "virtual").
    fn name(&self) -> &str;

    /// Matches a device attached to this bus with a registered driver.
    ///
    /// # Parameters
    /// - `device`: The device discovered on this bus.
    /// - `driver`: The driver candidate being evaluated.
    fn match_device(&self, device: &Arc<dyn Device>, driver: &Arc<dyn Driver>) -> bool;
}

/// PCI Bus implementation responsible for matching PCI devices and drivers.
pub struct PciBus;

impl Bus for PciBus {
    fn name(&self) -> &str {
        "pci"
    }

    fn match_device(&self, device: &Arc<dyn Device>, driver: &Arc<dyn Driver>) -> bool {
        if driver.bus_name() != "pci" {
            return false;
        }
        driver.match_device(device)
    }
}

/// Platform / System Bus implementation for built-in SoC and motherboard devices.
pub struct PlatformBus;

impl Bus for PlatformBus {
    fn name(&self) -> &str {
        "platform"
    }

    fn match_device(&self, device: &Arc<dyn Device>, driver: &Arc<dyn Driver>) -> bool {
        if driver.bus_name() != "platform" {
            return false;
        }
        driver.match_device(device)
    }
}

/// Virtual Bus implementation for pseudo-devices and synthetic hardware.
pub struct VirtualBus;

impl Bus for VirtualBus {
    fn name(&self) -> &str {
        "virtual"
    }

    fn match_device(&self, device: &Arc<dyn Device>, driver: &Arc<dyn Driver>) -> bool {
        if driver.bus_name() != "virtual" {
            return false;
        }
        driver.match_device(device)
    }
}

pub(crate) fn init_system_buses() -> Result<(), ostd::Error> {
    let _ = crate::device::manager::register_bus(Arc::new(PciBus));
    let _ = crate::device::manager::register_bus(Arc::new(PlatformBus));
    let _ = crate::device::manager::register_bus(Arc::new(VirtualBus));
    Ok(())
}

crate::module_init!(BUSES_INITCALL, "system_buses", init_system_buses);
