/// Device and driver registry — Bus-Driven Matching and driver kernel module integration.
use crate::device::bus::{Bus, PciBus, PlatformBus, VirtualBus};
use crate::device::device::{Device, DeviceType};
use crate::device::driver::Driver;
use crate::device::driver_module::DriverKernelModule;
use crate::fs::vfs::FileType;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use ostd::sync::SpinLock;

// ---------------------------------------------------------------------------
// Global registries
// ---------------------------------------------------------------------------

static BUSES: SpinLock<BTreeMap<String, Arc<dyn Bus>>> = SpinLock::new(BTreeMap::new());
static DRIVERS: SpinLock<BTreeMap<String, Arc<dyn Driver>>> = SpinLock::new(BTreeMap::new());
static DEVICES: SpinLock<BTreeMap<String, Arc<dyn Device>>> = SpinLock::new(BTreeMap::new());
static BUS_DEVICES: SpinLock<BTreeMap<String, Vec<Arc<dyn Device>>>> =
    SpinLock::new(BTreeMap::new());

// ---------------------------------------------------------------------------
// Bus Registration
// ---------------------------------------------------------------------------

/// Register a bus implementation with the global bus registry.
pub fn register_bus(bus: Arc<dyn Bus>) -> Result<(), ostd::Error> {
    let mut buses = BUSES.lock();
    let name = String::from(bus.name());
    if buses.contains_key(&name) {
        return Err(ostd::Error::InvalidArgs);
    }
    buses.insert(name, bus);
    Ok(())
}

// ---------------------------------------------------------------------------
// Driver Registration & Kernel Module Integration
// ---------------------------------------------------------------------------

/// Register a driver with the global driver registry.
///
/// **Bus-Driven Matching Integration**:
/// 1. Registers the driver into the driver registry.
/// 2. Automatically creates and registers a corresponding [`DriverKernelModule`]
///    in [`crate::modules`], tracking the driver as an active kernel module.
/// 3. Executes Bus-Driven Matching against all devices registered on the driver's bus.
pub fn register_driver(driver: Arc<dyn Driver>) -> Result<(), ostd::Error> {
    let name = String::from(driver.name());
    {
        let mut drivers = DRIVERS.lock();
        if drivers.contains_key(&name) {
            return Err(ostd::Error::InvalidArgs);
        }
        drivers.insert(name.clone(), driver.clone());
    }

    // Register driver as a KernelModule in crate::modules
    let module = Arc::new(DriverKernelModule::new(driver.clone()));
    let _ = crate::modules::register_module(module);
    let _ = crate::modules::load_module(driver.name());

    // Immediately attempt matching with devices already attached to driver's bus
    match_driver_with_bus(driver.clone());

    log::info!(
        "[device::manager] Registered driver '{}' (bus: {}) as kernel module",
        driver.name(),
        driver.bus_name()
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Device Registration
// ---------------------------------------------------------------------------

/// Register a generic device into devfs and the global device registry.
pub fn register_device(device: Arc<dyn Device>) -> Result<(), ostd::Error> {
    let name = String::from(device.name());

    {
        let mut devices = DEVICES.lock();
        if devices.contains_key(&name) {
            return Err(ostd::Error::InvalidArgs);
        }
        devices.insert(name.clone(), device.clone());
    }

    // Register device node in devfs VFS if it provides inode operations
    let file_type = match device.device_type() {
        DeviceType::Char => FileType::CharDevice,
        DeviceType::Block => FileType::BlockDevice,
        _ => FileType::CharDevice,
    };

    if let Some(inode) = device.inode_ops() {
        crate::fs::devfs::register_device(&name, file_type, 0o666, inode)?;
    }

    log::info!("[device::manager] Registered device node /dev/{}", name);

    Ok(())
}

/// Register a device attached to a specific bus (e.g. PCI, Platform, Virtual).
pub fn register_device_on_bus(bus_name: &str, device: Arc<dyn Device>) -> Result<(), ostd::Error> {
    register_device(device.clone())?;

    {
        let mut bus_devs = BUS_DEVICES.lock();
        bus_devs
            .entry(String::from(bus_name))
            .or_insert_with(Vec::new)
            .push(device.clone());
    }

    // Attempt matching with existing drivers on this bus
    match_device_with_bus(bus_name, &device);

    Ok(())
}

/// Unregister a device from devfs and the global registry.
pub fn unregister_device(name: &str) -> Result<(), ostd::Error> {
    {
        let mut devices = DEVICES.lock();
        if devices.remove(name).is_none() {
            return Err(ostd::Error::InvalidArgs);
        }
    }

    crate::fs::devfs::unregister_device(name)?;
    log::info!("[device::manager] Unregistered device /dev/{}", name);

    Ok(())
}

// ---------------------------------------------------------------------------
// Bus-Driven Matching Engine
// ---------------------------------------------------------------------------

/// Match a single driver with all devices registered on its target bus.
fn match_driver_with_bus(driver: Arc<dyn Driver>) {
    let buses = BUSES.lock();
    let bus = match buses.get(driver.bus_name()) {
        Some(bus) => bus,
        None => return,
    };

    let bus_devs = BUS_DEVICES.lock();
    if let Some(devices) = bus_devs.get(driver.bus_name()) {
        for device in devices {
            if bus.match_device(device, &driver) {
                log::info!(
                    "[bus_matching] Bus '{}' matched device '{}' with driver '{}'",
                    bus.name(),
                    device.name(),
                    driver.name()
                );
                let _ = driver.probe_device(device.clone());
            }
        }
    }
}

/// Match a single newly registered device with registered drivers on its bus.
fn match_device_with_bus(bus_name: &str, device: &Arc<dyn Device>) {
    let buses = BUSES.lock();
    let bus = match buses.get(bus_name) {
        Some(bus) => bus,
        None => return,
    };

    let drivers = DRIVERS.lock();
    for driver in drivers.values() {
        if bus.match_device(device, driver) {
            log::info!(
                "[bus_matching] Bus '{}' matched device '{}' with driver '{}'",
                bus.name(),
                device.name(),
                driver.name()
            );
            let _ = driver.probe_device(device.clone());
        }
    }
}

/// Execute Bus-Driven Matching across all registered buses, devices, and drivers.
pub fn run_bus_matching() -> usize {
    let buses = BUSES.lock();
    let drivers = DRIVERS.lock();
    let bus_devs = BUS_DEVICES.lock();
    let mut matches = 0;

    for (bus_name, bus) in buses.iter() {
        if let Some(devices) = bus_devs.get(bus_name) {
            for device in devices {
                for driver in drivers.values() {
                    if bus.match_device(device, driver) {
                        log::info!(
                            "[bus_matching] Bus '{}' bound device '{}' -> driver '{}'",
                            bus_name,
                            device.name(),
                            driver.name()
                        );
                        let _ = driver.probe_device(device.clone());
                        matches += 1;
                    }
                }
            }
        }
    }

    matches
}
