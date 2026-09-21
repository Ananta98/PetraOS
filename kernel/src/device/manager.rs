//! Global Device Manager
//!
//! Maintains a registry of all discovered hardware devices.
//! Devices are stored by registration order and keyed by name for fast lookup.

use super::device::{Device, DeviceType};
use crate::fs::devfs;
use crate::sync::Mutex;
use crate::sync::rwlock::RwLock;
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;

/// The global kernel device manager.
pub static DEVICE_MANAGER: RwLock<DeviceManager> = RwLock::new(DeviceManager::new());

pub struct DeviceManager {
    /// Ordered list of all registered devices.
    devices: Vec<Arc<Mutex<Box<dyn Device>>>>,
    /// Name-indexed lookup table for O(log n) access by device name.
    by_name: BTreeMap<&'static str, Arc<Mutex<Box<dyn Device>>>>,
}

impl DeviceManager {
    pub const fn new() -> Self {
        Self {
            devices: Vec::new(),
            by_name: BTreeMap::new(),
        }
    }

    /// Register a device with the manager.
    ///
    /// The device is indexed by both its human-readable `name()` and standard
    /// `/dev` node name `dev_name()` (if specified).
    pub fn register(&mut self, device: Arc<Mutex<Box<dyn Device>>>) {
        let (name, dev_name) = {
            let guard = device.lock();
            (guard.name(), guard.dev_name())
        };
        self.by_name.entry(name).or_insert_with(|| device.clone());
        if let Some(dev_n) = dev_name {
            self.by_name.entry(dev_n).or_insert_with(|| device.clone());
        }
        self.devices.push(device.clone());
        devfs::sync_device_to_devfs(&device);
    }

    /// Unregister a device from the manager and DevFS by its name or dev_name.
    pub fn unregister(&mut self, name: &str) -> Option<Arc<Mutex<Box<dyn Device>>>> {
        let idx = self.devices.iter().position(|d| {
            let guard = d.lock();
            guard.name() == name || guard.dev_name() == Some(name)
        })?;
        let dev = self.devices.remove(idx);
        let (d_name, d_vfs) = {
            let guard = dev.lock();
            (guard.name(), guard.dev_name())
        };
        self.by_name.remove(d_name);
        if let Some(vfs) = d_vfs {
            self.by_name.remove(vfs);
            devfs::unregister_dev_node(vfs);
        }
        Some(dev)
    }

    /// Borrow the ordered slice of all registered devices.
    ///
    /// Prefer `get_by_name` or `get_by_type` for targeted lookups.
    pub fn devices(&self) -> &[Arc<Mutex<Box<dyn Device>>>] {
        &self.devices
    }

    /// Look up a device by its unique name or `/dev` node name in O(log n).
    pub fn get_by_name(&self, name: &str) -> Option<Arc<Mutex<Box<dyn Device>>>> {
        self.by_name.get(name).cloned()
    }

    /// Return all devices of the given type.
    pub fn get_by_type(&self, dev_type: DeviceType) -> Vec<Arc<Mutex<Box<dyn Device>>>> {
        self.devices
            .iter()
            .filter(|d| d.lock().dev_type() == dev_type)
            .cloned()
            .collect()
    }
}
