//! NVMe Device and Driver Trait Abstractions

use crate::device::{Device, DriverError};
use crate::sync::Mutex;
use alloc::boxed::Box;
use alloc::sync::Arc;

#[derive(Default)]
pub struct NvmeModuleDriver;

impl crate::device::Driver for NvmeModuleDriver {
    fn name(&self) -> &'static str {
        "nvme"
    }

    fn bus_name(&self) -> &'static str {
        "pci"
    }

    fn description(&self) -> &'static str {
        "NVM Express Block Device Driver"
    }

    fn probe(&self) -> Result<(), DriverError> {
        if let Some(mut nvme) = super::NvmeDriver::find_and_init() {
            nvme.init()?;
            let device_ref: Arc<Mutex<Box<dyn Device>>> =
                Arc::new(Mutex::new(Box::new(nvme)));
            crate::device::DEVICE_MANAGER.write().register(device_ref);
            log::info!("[NVMe Module] Probed and registered NVMe Controller to DEVICE_MANAGER");
            Ok(())
        } else {
            Err(DriverError::InitFailed)
        }
    }
}
