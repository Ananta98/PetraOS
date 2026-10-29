// SPDX-License-Identifier: GPL-2.0

//! Device classes.
//!
//! A [`Class`] is what a device looks like to user space, independent of how
//! it is attached to the system: a block device, a terminal, a network
//! interface. A [`ClassHandle`] is the single registry for one class and is
//! usually created as a static by the subsystem that defines the class; it
//! replaces hand-rolled global device lists.

use alloc::{sync::Arc, vec::Vec};
use spin::RwLock;

use super::base::{AnyDevice, DeviceBase};
use super::privacy::Sealed;
use crate::uapi::errno::{Errno, Result};

/// A category of interchangeable devices.
pub trait Class: Send + Sync + 'static {
    /// The payload interface every device of this class provides.
    type Device: ?Sized + Send + Sync;
}

/// A registered class owning its devices.
pub struct ClassHandle<C: Class> {
    devices: RwLock<Vec<Arc<ClassDevice<C>>>>,
}

impl<C: Class> ClassHandle<C> {
    /// Creates a class handle with no devices, suitable for a static.
    pub const fn new() -> Self {
        Self {
            devices: RwLock::new(Vec::new()),
        }
    }

    /// Registers a device named `name` with the class.
    pub fn register(&self, name: &str, payload: Arc<C::Device>) -> Result<Arc<ClassDevice<C>>> {
        let device: Arc<ClassDevice<C>> = Arc::new(ClassDevice::new(name, payload));
        device.base.register()?;
        self.devices
            .write()
            .push(Arc::<ClassDevice<C>>::clone(&device));
        Ok(device)
    }

    /// Unregisters the device named `name`.
    pub fn unregister(&self, name: &str) -> Result<Arc<ClassDevice<C>>> {
        let device = self.find(name).ok_or(Errno::ENODEV)?;
        device.base.unregister()?;
        self.devices
            .write()
            .retain(|d| !Arc::ptr_eq(d, &device));
        Ok(device)
    }

    /// Returns a snapshot of the devices in the class.
    pub fn devices(&self) -> Vec<Arc<ClassDevice<C>>> {
        self.devices.read().clone()
    }

    /// Returns the device named `name`, if present.
    pub fn find(&self, name: &str) -> Option<Arc<ClassDevice<C>>> {
        self.devices
            .read()
            .iter()
            .find(|d| d.name() == name)
            .cloned()
    }

    /// Returns the payload of the device named `name`, if present, without cloning the full list.
    pub fn find_payload(&self, name: &str) -> Option<Arc<C::Device>> {
        self.devices
            .read()
            .iter()
            .find(|d| d.name() == name)
            .map(|d| Arc::clone(&d.payload))
    }

    /// Returns a snapshot of the payloads backing the class devices.
    pub fn payloads(&self) -> Vec<Arc<C::Device>> {
        self.devices
            .read()
            .iter()
            .map(|d| Arc::clone(&d.payload))
            .collect()
    }
}

/// A device in class `C`, whose payload is the device face user space sees.
pub struct ClassDevice<C: Class> {
    base: DeviceBase,
    payload: Arc<C::Device>,
}

impl<C: Class> ClassDevice<C> {
    fn new(name: &str, payload: Arc<C::Device>) -> Self {
        Self {
            base: DeviceBase::new(name),
            payload,
        }
    }

    /// Returns the payload backing this device.
    pub fn payload(&self) -> &Arc<C::Device> {
        &self.payload
    }
}

impl<C: Class> core::ops::Deref for ClassDevice<C> {
    type Target = C::Device;

    fn deref(&self) -> &Self::Target {
        &self.payload
    }
}

impl<C: Class> AnyDevice for ClassDevice<C> {
    fn base(&self) -> &DeviceBase {
        &self.base
    }
}

impl<C: Class> Sealed for ClassDevice<C> {}
