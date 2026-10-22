// SPDX-License-Identifier: GPL-2.0

//! Buses, their devices, and drivers.
//!
//! A [`Bus`] owns the devices discovered on it and decides which driver
//! serves which device. Drivers register through a [`BusHandle`] and are
//! offered every unbound device; likewise a newly added device is offered to
//! every registered driver. The first driver whose [`Driver::on_probe`]
//! succeeds wins the binding.

use alloc::{sync::Arc, vec::Vec};
use spin::{Mutex, RwLock};

use super::base::{AnyDevice, DeviceBase};
use super::privacy::Sealed;
use crate::uapi::errno::{Errno, Result};

/// The mechanism that discovers devices of one kind and matches drivers to
/// them.
pub trait Bus: Send + Sync + 'static {
    /// The payload data a device on this bus exposes to its drivers.
    type Device: Send + Sync;

    /// The matching declaration a driver registers with this bus.
    type MatchData: ?Sized;

    /// Returns whether `driver`'s match data accepts `device`.
    fn matches(&self, device: &Self::Device, driver: &Self::MatchData) -> bool;
}

/// A driver for devices on bus `B`.
pub trait Driver<B: Bus>: Send + Sync + 'static {
    /// The driver name; unique on its bus.
    fn name(&self) -> &str;

    /// The devices this driver accepts, in the bus's own terms.
    fn match_data(&self) -> &B::MatchData;

    /// Initializes a matched, unbound device.
    fn on_probe(&self, device: &BusDevice<B>) -> Result<()>;

    /// Releases resources `on_probe` acquired; runs when the driver is
    /// unregistered or the device is rebound.
    fn on_release(&self, _device: &BusDevice<B>) {}
}

/// A registered bus that owns its devices and drivers.
pub struct BusHandle<B: Bus> {
    bus: B,
    devices: RwLock<Vec<Arc<BusDevice<B>>>>,
    drivers: RwLock<Vec<Arc<DriverHandle<B>>>>,
}

impl<B: Bus> BusHandle<B> {
    /// Creates a bus handle for `bus`, suitable for a static.
    pub const fn new(bus: B) -> Self {
        Self {
            bus,
            devices: RwLock::new(Vec::new()),
            drivers: RwLock::new(Vec::new()),
        }
    }

    /// Returns the bus this handle brings up.
    pub fn bus(&self) -> &B {
        &self.bus
    }

    /// Registers a discovered device named `name` and offers it to every
    /// registered driver.
    ///
    /// Binding failures are best effort: a device without a matching driver
    /// stays on the bus for a later driver to claim it.
    pub fn add_device(&self, name: &str, payload: B::Device) -> Result<Arc<BusDevice<B>>> {
        let device: Arc<BusDevice<B>> = Arc::new(BusDevice::new(name, payload));
        device.base.register()?;
        self.devices
            .write()
            .push(Arc::<BusDevice<B>>::clone(&device));
        let _ = self.probe(&device);
        Ok(device)
    }

    /// Unregisters the device named `name`, releasing its driver first.
    pub fn remove_device(&self, name: &str) -> Result<Arc<BusDevice<B>>> {
        let device = self
            .find_device(name)
            .ok_or(Errno::ENODEV)?;
        if let Some(driver) = device.driver() {
            self.unbind(&device, &driver)?;
        }
        device.base.unregister()?;
        self.devices
            .write()
            .retain(|d| !Arc::ptr_eq(d, &device));
        Ok(device)
    }

    /// Registers a driver and offers it every unbound device.
    ///
    /// # Concurrency
    ///
    /// Only one driver of a given name can be registered; concurrent
    /// registrations are not supported (drivers register from boot initcalls).
    pub fn register_driver(&self, driver: Arc<dyn Driver<B>>) -> Result<Arc<DriverHandle<B>>> {
        let handle = Arc::new(DriverHandle::new(driver));
        if self.drivers.read().iter().any(|d| d.name() == handle.name()) {
            return Err(Errno::EEXIST);
        }
        self.drivers.write().push(handle.clone());
        for device in self.devices() {
            if device.driver().is_none() {
                let _ = self.probe(&device);
            }
        }
        Ok(handle)
    }

    /// Unregisters the driver named `name` and releases its bound devices.
    pub fn unregister_driver(&self, name: &str) -> Result<()> {
        let driver = {
            let mut drivers = self.drivers.write();
            let position = drivers
                .iter()
                .position(|d| d.name() == name)
                .ok_or(Errno::ENOENT)?;
            drivers.remove(position)
        };
        for device in self.devices() {
            if device.driver().is_some_and(|d| Arc::ptr_eq(&d, &driver)) {
                driver.on_release(&device);
                device.clear_driver();
            }
        }
        Ok(())
    }

    /// Offers `device` to the registered drivers that match it; the first
    /// successful [`Driver::on_probe`] binds it.
    ///
    /// Returns `ENODEV` when no driver matched, or the last failing driver's
    /// error when a matching driver rejected the device.
    pub fn probe(&self, device: &Arc<BusDevice<B>>) -> Result<()> {
        let drivers = self.drivers.read().clone();
        let mut matched = false;
        let mut last_error = Errno::ENODEV;
        for driver in drivers {
            if self.bus.matches(device.payload(), driver.match_data()) {
                matched = true;
                match self.try_bind(device, &driver) {
                    Ok(()) => return Ok(()),
                    Err(error) => last_error = error,
                }
            }
        }
        Err(if matched { last_error } else { Errno::ENODEV })
    }

    /// Returns a snapshot of the devices on the bus.
    pub fn devices(&self) -> Vec<Arc<BusDevice<B>>> {
        self.devices.read().clone()
    }

    /// Returns the device named `name`, if present.
    pub fn find_device(&self, name: &str) -> Option<Arc<BusDevice<B>>> {
        self.devices
            .read()
            .iter()
            .find(|d| d.name() == name)
            .cloned()
    }

    /// Binds `device` to `driver` if the bus matches them.
    fn try_bind(&self, device: &Arc<BusDevice<B>>, driver: &Arc<DriverHandle<B>>) -> Result<()> {
        let _binding = device.bind_lock.lock();
        if !self.bus.matches(device.payload(), driver.match_data()) {
            return Err(Errno::ENODEV);
        }
        if device.driver().is_some() {
            return Err(Errno::EBUSY);
        }
        driver.on_probe(device)?;
        device.set_driver(driver.clone());
        Ok(())
    }

    /// Removes the binding between `device` and `driver`.
    fn unbind(&self, device: &Arc<BusDevice<B>>, driver: &Arc<DriverHandle<B>>) -> Result<()> {
        let _binding = device.bind_lock.lock();
        if !device.driver().is_some_and(|d| Arc::ptr_eq(&d, driver)) {
            return Err(Errno::ENODEV);
        }
        driver.on_release(device);
        device.clear_driver();
        Ok(())
    }
}

/// A registered driver on bus `B`.
pub struct DriverHandle<B: Bus> {
    driver: Arc<dyn Driver<B>>,
}

impl<B: Bus> DriverHandle<B> {
    fn new(driver: Arc<dyn Driver<B>>) -> Self {
        Self { driver }
    }
}

impl<B: Bus> core::ops::Deref for DriverHandle<B> {
    type Target = dyn Driver<B>;

    fn deref(&self) -> &Self::Target {
        self.driver.as_ref()
    }
}

/// A device discovered on bus `B`, dedereferenced to its bus-specific
/// payload and carrying the driver currently bound to it.
pub struct BusDevice<B: Bus> {
    base: DeviceBase,
    payload: B::Device,
    driver: RwLock<Option<Arc<DriverHandle<B>>>>,
    /// Serializes binding and unbinding so a probe never races a release.
    bind_lock: Mutex<()>,
}

impl<B: Bus> BusDevice<B> {
    fn new(name: &str, payload: B::Device) -> Self {
        Self {
            base: DeviceBase::new(name),
            payload,
            driver: RwLock::new(None),
            bind_lock: Mutex::new(()),
        }
    }

    /// Returns the bus-specific payload.
    pub fn payload(&self) -> &B::Device {
        &self.payload
    }

    /// Returns the driver currently bound, if any.
    pub fn driver(&self) -> Option<Arc<DriverHandle<B>>> {
        self.driver.read().clone()
    }

    fn set_driver(&self, driver: Arc<DriverHandle<B>>) {
        *self.driver.write() = Some(driver);
    }

    fn clear_driver(&self) {
        *self.driver.write() = None;
    }
}

impl<B: Bus> core::ops::Deref for BusDevice<B> {
    type Target = B::Device;

    fn deref(&self) -> &Self::Target {
        &self.payload
    }
}

impl<B: Bus> AnyDevice for BusDevice<B> {
    fn base(&self) -> &DeviceBase {
        &self.base
    }
}

impl<B: Bus> Sealed for BusDevice<B> {}
