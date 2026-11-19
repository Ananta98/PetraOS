// SPDX-License-Identifier: MPL-2.0
//
// Ported from Asterinas `aster-device` (`kernel/core/comps/device/src`):
// https://github.com/asterinas/asterinas/tree/e36b5b3b759e616febf2c8f0e88d8cb2c103f266/kernel/core/comps/device/src
// Copyright (c) Asterinas contributors. Adapted for PetraOS.

//! Device classes, their devices, and observers.
//!
//! A class is what a device looks like to user space, regardless of how it is attached:
//! a block device, a terminal, a character device, a network interface.
//! A [`Class`] implementation supplies the per-device payload type,
//! the `/dev` naming policy, and class-wide attributes.
//! Registering it yields a [`ClassHandle`], which owns `/sys/class/<name>` and the member list,
//! and lets [`ClassObserver`]s subscribe to membership changes.

mod device;
mod observer;

use alloc::{sync::Arc, vec::Vec};

use ostd::sync::Mutex;

pub use self::{
    device::{ClassDevice, ClassDeviceBuilder},
    observer::ClassObserver,
};
use crate::drivers::core::common::{
    AnyDevice, DevNode, DeviceInternals, Error, Result, SysStr,
    attr::Attr,
    node::Dir,
    registry,
    subsystem::SubsystemOps,
};

/// A kind of class.
pub trait Class: Sized + Send + Sync + 'static {
    /// The class name: the directory under `/sys/class`.
    const NAME: &'static str;

    /// The payload type of devices in this class.
    type Device: Send + Sync + 'static;

    /// Overrides the `/dev` node name or mode of a device.
    fn devnode(&self, _dev: &ClassDevice<Self>) -> Option<DevNode> {
        None
    }

    /// Returns the attributes shared by all devices in this class.
    fn dev_attrs(&self) -> &'static [Attr<ClassDevice<Self>>] {
        &[]
    }

    /// Whether a device of this class placed under a class device still gets its own glue directory.
    const KEEPS_GLUE_DIR: bool = false;
}

/// Registers a class, creating `/sys/class/<name>`.
///
/// # Panics
///
/// Panics if the class name is not a valid sysfs node name.
pub fn register<C: Class>(class: C) -> Result<Arc<ClassHandle<C>>> {
    let dir = Dir::new(SysStr::from(C::NAME));
    registry::get().class_root().attach_dir(&dir)?;
    registry::get().mirror_class(C::NAME, dir.clone());

    let handle = Arc::new(ClassHandle {
        class,
        dir,
        devices: Mutex::new(Vec::new()),
        observers: Mutex::new(Vec::new()),
        membership: Mutex::new(()),
    });
    registry::get().keep_subsystem(handle.clone());
    Ok(handle)
}

/// A registered class.
pub struct ClassHandle<C: Class> {
    class: C,
    dir: Arc<Dir>,
    devices: Mutex<Vec<Arc<ClassDevice<C>>>>,
    observers: Mutex<Vec<Arc<dyn ClassObserver<C>>>>,
    /// Serializes membership changes with observer registration so an observer
    /// sees each member exactly once.
    membership: Mutex<()>,
}

impl<C: Class> ClassHandle<C> {
    /// Registers an observer and announces the current members through [`on_device_added`].
    ///
    /// [`on_device_added`]: ClassObserver::on_device_added
    pub fn register_observer(&self, observer: Arc<dyn ClassObserver<C>>) -> Result<()> {
        let _guard = self.membership.lock();
        {
            let mut observers = self.observers.lock();
            if observers.iter().any(|o| Arc::ptr_eq(o, &observer)) {
                return Err(Error::AlreadyAdded);
            }
            observers.push(observer.clone());
        }
        for dev in self.devices() {
            observer.on_device_added(&dev);
        }
        Ok(())
    }

    /// Unregisters an observer and announces the current members through [`on_device_removed`].
    ///
    /// [`on_device_removed`]: ClassObserver::on_device_removed
    pub fn unregister_observer(&self, observer: &Arc<dyn ClassObserver<C>>) -> Result<()> {
        let _guard = self.membership.lock();
        let removed = {
            let mut observers = self.observers.lock();
            let before = observers.len();
            observers.retain(|o| !Arc::ptr_eq(o, observer));
            observers.len() != before
        };
        if !removed {
            return Err(Error::NotFound);
        }
        for dev in self.devices() {
            observer.on_device_removed(&dev);
        }
        Ok(())
    }

    /// Returns the class itself.
    pub fn class(&self) -> &C {
        &self.class
    }

    /// Returns the devices currently in the class.
    pub fn devices(&self) -> Vec<Arc<ClassDevice<C>>> {
        self.devices.lock().clone()
    }

    /// Finds a device in the class by name.
    pub fn find_device(&self, name: &str) -> Option<Arc<ClassDevice<C>>> {
        self.devices
            .lock()
            .iter()
            .find(|d| d.base().name().as_ref() == name)
            .cloned()
    }

    fn observers(&self) -> Vec<Arc<dyn ClassObserver<C>>> {
        self.observers.lock().clone()
    }
}

impl<C: Class> SubsystemOps for ClassHandle<C> {
    fn name(&self) -> &'static str {
        C::NAME
    }

    fn dir(&self) -> Arc<Dir> {
        self.dir.clone()
    }

    fn index_dir(&self) -> Arc<Dir> {
        self.dir.clone()
    }

    fn keeps_glue_dir(&self) -> bool {
        C::KEEPS_GLUE_DIR
    }

    fn on_added(&self, dev: &Arc<dyn AnyDevice>) {
        let dev = dev
            .as_any()
            .downcast_ref::<ClassDevice<C>>()
            .expect("a class device reports its own class as its subsystem");
        let dev = dev.this();

        let _guard = self.membership.lock();
        self.devices.lock().push(dev.clone());
        for observer in self.observers() {
            observer.on_device_added(&dev);
        }
    }

    fn on_removed(&self, dev: &Arc<dyn AnyDevice>) {
        let dev = dev
            .as_any()
            .downcast_ref::<ClassDevice<C>>()
            .expect("a class device reports its own class as its subsystem");
        let dev = dev.this();

        let _guard = self.membership.lock();
        let is_member = self.devices.lock().iter().any(|d| Arc::ptr_eq(d, &dev));
        if !is_member {
            return;
        }
        for observer in self.observers() {
            observer.on_device_removed(&dev);
        }
        self.devices.lock().retain(|d| !Arc::ptr_eq(d, &dev));
    }
}

impl<C: Class> core::fmt::Debug for ClassHandle<C> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ClassHandle")
            .field("name", &C::NAME)
            .finish()
    }
}
