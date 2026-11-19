// SPDX-License-Identifier: MPL-2.0
//
// Ported from Asterinas `aster-device` (`kernel/core/comps/device/src`):
// https://github.com/asterinas/asterinas/tree/e36b5b3b759e616febf2c8f0e88d8cb2c103f266/kernel/core/comps/device/src
// Copyright (c) Asterinas contributors. Adapted for PetraOS.

//! Drivers for devices on a bus.
//!
//! A [`Driver`] supplies the matching data and callbacks used by [`BusHandle`].
//! Registering it creates a [`DriverHandle`],
//! which tracks its binding state and provides the driver's sysfs attribute directory.

use alloc::{
    boxed::Box,
    string::ToString,
    sync::{Arc, Weak},
    vec::Vec,
};
use core::ops::Deref;

use ostd::sync::Mutex;

use crate::drivers::core::{
    bus::{Bus, BusDevice, BusHandle},
    common::{
        DeviceInternals, Error, Result, SysPerms, SysStr,
        attr::Attr,
        node::{self, Dir, DirAttrOps},
    },
};

/// A driver for devices on bus `B`.
///
/// # Concurrency
///
/// For each device, [`on_probe`], [`on_release`],
/// and the attribute callbacks returned by [`dev_attrs`] run serially.
/// To avoid deadlocks, these callbacks must not synchronously bind, unbind,
/// or remove this device, unregister this driver, or invoke a driver attribute
/// callback on this device. Registering and removing child devices is allowed.
///
/// [`on_probe`]: Self::on_probe
/// [`on_release`]: Self::on_release
/// [`dev_attrs`]: Self::dev_attrs
pub trait Driver<B: Bus>: Send + Sync + 'static {
    /// The driver name: the directory under `/sys/bus/<bus>/drivers`.
    fn name(&self) -> &str;

    /// The devices this driver accepts, in the bus's terms.
    fn match_data(&self) -> &B::MatchData;

    /// Initializes a device accepted by [`Bus::matches`].
    fn on_probe(&self, dev: &Arc<BusDevice<B>>) -> Result<()>;

    /// Releases the resources held by this driver for the device.
    fn on_release(&self, _dev: &Arc<BusDevice<B>>) {}

    /// Returns the attributes exposed by devices while bound to this driver.
    fn dev_attrs(&self) -> &'static [Attr<BusDevice<B>>] {
        &[]
    }

    /// Returns whether to omit the driver's sysfs `bind` and `unbind` attributes.
    fn suppress_bind_attrs(&self) -> bool {
        false
    }
}

/// A registered driver.
///
/// Dereferences to the driver itself.
pub struct DriverHandle<B: Bus> {
    driver: Box<dyn Driver<B>>,
    dev_attrs: &'static [Attr<BusDevice<B>>],
    dir: Arc<Dir>,
    state: Mutex<DriverState<B>>,
    bus: Weak<BusHandle<B>>,
    weak: Weak<Self>,
}

impl<B: Bus> DriverHandle<B> {
    /// Creates a driver handle and its attribute directory.
    ///
    /// # Panics
    ///
    /// Panics if the driver name is not a valid sysfs node name.
    pub(crate) fn new(driver: Box<dyn Driver<B>>, bus: Weak<BusHandle<B>>) -> Result<Arc<Self>> {
        let name = SysStr::from(driver.name().to_string());
        let suppress_bind_attrs = driver.suppress_bind_attrs();
        let dir = if suppress_bind_attrs {
            Dir::new(name)
        } else {
            Dir::with_attrs(
                name,
                &[("bind", SysPerms::OWNER_W), ("unbind", SysPerms::OWNER_W)],
            )?
        };
        let dev_attrs = driver.dev_attrs();
        let handle = Arc::new_cyclic(|weak| DriverHandle {
            driver,
            dev_attrs,
            dir: dir.clone(),
            state: Mutex::new(DriverState::new()),
            bus,
            weak: weak.clone(),
        });
        if !suppress_bind_attrs {
            dir.set_ops(Box::new(DriverDirOps {
                driver: handle.weak.clone(),
            }));
        }
        Ok(handle)
    }

    /// Returns the device attributes saved when the driver was registered.
    pub(crate) fn dev_attrs(&self) -> &'static [Attr<BusDevice<B>>] {
        self.dev_attrs
    }

    /// Returns the driver's attribute directory.
    pub(crate) fn dir(&self) -> &Arc<Dir> {
        &self.dir
    }

    /// Returns the lock protecting registration and binding records.
    pub(crate) fn state(&self) -> &Mutex<DriverState<B>> {
        &self.state
    }

    /// Returns the bus the driver is registered with.
    fn bus(&self) -> Option<Arc<BusHandle<B>>> {
        self.bus.upgrade()
    }
}

impl<B: Bus> Deref for DriverHandle<B> {
    type Target = dyn Driver<B>;

    fn deref(&self) -> &dyn Driver<B> {
        self.driver.as_ref()
    }
}

/// Removes the `driver` link of `dev` and the device entry in the driver's directory,
/// the two links created during binding.
pub(crate) fn remove_driver_links<B: Bus>(dev: &Arc<BusDevice<B>>, driver: &Arc<DriverHandle<B>>) {
    node::remove_link(dev.base(), "driver");
    node::remove_link(driver.dir.as_ref(), dev.base().name());
}

/// A driver's registration state includes its binding records.
///
/// Lock order: device binding lock, then driver state lock.
/// Never hold the driver state lock across callbacks or while acquiring a device binding lock.
pub(crate) enum DriverState<B: Bus> {
    /// Accepts bindings; successful bindings move from `binding` to `bound` under the state lock.
    Registered {
        bound: Vec<Arc<BusDevice<B>>>,
        binding: Vec<Arc<BusDevice<B>>>,
    },
    /// No new bindings are allowed; cleanup of existing bindings may still be in progress.
    Unregistered,
}

impl<B: Bus> DriverState<B> {
    pub(crate) fn start_binding(&mut self, dev: &Arc<BusDevice<B>>) -> Result<()> {
        let Self::Registered { binding, .. } = self else {
            return Err(Error::DriverUnregistered);
        };
        binding.push(dev.clone());
        Ok(())
    }

    pub(crate) fn finish_binding(&mut self, dev: &Arc<BusDevice<B>>) {
        if let Self::Registered { bound, binding } = self {
            binding.retain(|d| !Arc::ptr_eq(d, dev));
            bound.push(dev.clone());
        }
    }

    pub(crate) fn abort_binding(&mut self, dev: &Arc<BusDevice<B>>) {
        if let Self::Registered { binding, .. } = self {
            binding.retain(|d| !Arc::ptr_eq(d, dev));
        }
    }

    pub(crate) fn forget_bound(&mut self, dev: &Arc<BusDevice<B>>) {
        if let Self::Registered { bound, .. } = self {
            bound.retain(|d| !Arc::ptr_eq(d, dev));
        }
    }

    pub(crate) fn set_unregistered(&mut self) -> Result<Vec<Arc<BusDevice<B>>>> {
        match core::mem::replace(self, Self::Unregistered) {
            Self::Registered { mut bound, binding } => {
                bound.extend(binding);
                Ok(bound)
            }
            Self::Unregistered => Err(Error::NotFound),
        }
    }

    fn new() -> Self {
        Self::Registered {
            bound: Vec::new(),
            binding: Vec::new(),
        }
    }
}

/// The `bind` and `unbind` files of a driver directory.
struct DriverDirOps<B: Bus> {
    driver: Weak<DriverHandle<B>>,
}

impl<B: Bus> DirAttrOps for DriverDirOps<B> {
    fn store(&self, name: &str, value: &str) -> Result<()> {
        let driver = self.driver.upgrade().ok_or(Error::NotFound)?;
        let bus = driver.bus().ok_or(Error::NotFound)?;
        let dev = bus.find_device(value.trim()).ok_or(Error::NotFound)?;
        match name {
            "bind" => bus.bind(&dev, &driver),
            "unbind" => bus.unbind_from(&dev, &driver),
            _ => Err(Error::NotFound),
        }
    }
}

impl<B: Bus> core::fmt::Debug for DriverHandle<B> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DriverHandle")
            .field("name", &self.driver.name())
            .finish()
    }
}
