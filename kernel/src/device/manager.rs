//! Global Device Manager

use super::device::Device;
use crate::sync::spinlock::Spinlock;
use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;

pub static DEVICE_MANAGER: Spinlock<DeviceManager> = Spinlock::new(DeviceManager::new());

pub struct DeviceManager {
    devices: Vec<Arc<Spinlock<Box<dyn Device>>>>,
}

impl DeviceManager {
    pub const fn new() -> Self {
        Self {
            devices: Vec::new(),
        }
    }

    pub fn register(&mut self, device: Arc<Spinlock<Box<dyn Device>>>) {
        self.devices.push(device);
    }

    pub fn get_devices(&self) -> Vec<Arc<Spinlock<Box<dyn Device>>>> {
        self.devices.clone()
    }
}
