/// Kernel module adapter for drivers.
///
/// Wraps any [`Driver`] into a [`KernelModule`] so that driver registration
/// automatically registers and tracks a corresponding kernel module in `crate::modules`.

use crate::device::driver::Driver;
use crate::modules::{KernelModule, ModuleInfo, ModuleState};
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;

/// Adapter struct exposing a [`Driver`] as a [`KernelModule`].
pub struct DriverKernelModule {
    driver: Arc<dyn Driver>,
}

impl DriverKernelModule {
    /// Create a new kernel module adapter wrapping the specified driver.
    pub fn new(driver: Arc<dyn Driver>) -> Self {
        Self { driver }
    }
}

impl KernelModule for DriverKernelModule {
    fn info(&self) -> ModuleInfo {
        ModuleInfo {
            name: format!("driver_{}", self.driver.name()),
            version: String::from("1.0.0"),
            author: String::from("PetraOS Device Framework"),
            description: String::from(self.driver.description()),
            license: String::from("GPL-2.0"),
            state: ModuleState::Registered,
        }
    }

    fn init(&self) -> Result<(), ostd::Error> {
        log::info!(
            "[driver_module] Initializing driver module: driver_{}",
            self.driver.name()
        );
        self.driver.probe()
    }

    fn exit(&self) -> Result<(), ostd::Error> {
        log::info!(
            "[driver_module] Unloaded driver module: driver_{}",
            self.driver.name()
        );
        Ok(())
    }
}
