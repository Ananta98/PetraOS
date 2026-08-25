//! Global Device Manager
//!
//! Maintains a registry of all discovered hardware devices.
//! Devices are stored by registration order and keyed by name for fast lookup.

use super::device::{Device, DeviceType};
use crate::sync::rwlock::RwLock;
use crate::sync::Mutex;
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
    /// The device's `name()` method must return a unique `'static` string.
    /// Devices registered first take priority in `get_by_name` for duplicate names.
    pub fn register(&mut self, device: Arc<Mutex<Box<dyn Device>>>) {
        let name = device.lock().name();
        self.by_name.entry(name).or_insert_with(|| device.clone());
        self.devices.push(device);
    }

    /// Borrow the ordered slice of all registered devices.
    ///
    /// Prefer `get_by_name` or `get_by_type` for targeted lookups.
    pub fn devices(&self) -> &[Arc<Mutex<Box<dyn Device>>>] {
        &self.devices
    }

    /// Look up a device by its unique name in O(log n).
    pub fn get_by_name(
        &self,
        name: &'static str,
    ) -> Option<Arc<Mutex<Box<dyn Device>>>> {
        self.by_name.get(name).cloned()
    }

    /// Return all devices of the given type.
    pub fn get_by_type(
        &self,
        dev_type: DeviceType,
    ) -> Vec<Arc<Mutex<Box<dyn Device>>>> {
        self.devices
            .iter()
            .filter(|d| d.lock().dev_type() == dev_type)
            .cloned()
            .collect()
    }
}
