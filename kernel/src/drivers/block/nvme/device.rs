//! NVMe Device and Driver Trait Abstractions

use crate::device::{BlockDevice, Device, DeviceType, DriverError};
use crate::sync::spinlock::Spinlock;
use alloc::boxed::Box;
use alloc::sync::Arc;
use super::NVME_DRIVER;

pub struct NvmeDeviceRef;

impl Device for NvmeDeviceRef {
    fn dev_type(&self) -> DeviceType {
        DeviceType::Block
    }

    fn name(&self) -> &'static str {
        "NVMe Controller"
    }

    fn dev_name(&self) -> Option<&'static str> {
        Some("nvme0n1")
    }

    fn init(&mut self) -> Result<(), DriverError> {
        if let Some(ref mut drv) = *NVME_DRIVER.lock() {
            drv.init()
        } else {
            Err(DriverError::Unsupported)
        }
    }

    fn as_block_device(&self) -> Option<&dyn BlockDevice> {
        Some(self)
    }

    fn as_block_device_mut(&mut self) -> Option<&mut dyn BlockDevice> {
        Some(self)
    }
}

impl BlockDevice for NvmeDeviceRef {
    fn read_block(&mut self, block_id: u64, buf: &mut [u8]) -> Result<usize, DriverError> {
        if let Some(ref mut drv) = *NVME_DRIVER.lock() {
            drv.read_block(block_id, buf)
        } else {
            Err(DriverError::Unsupported)
        }
    }

    fn write_block(&mut self, block_id: u64, buf: &[u8]) -> Result<usize, DriverError> {
        if let Some(ref mut drv) = *NVME_DRIVER.lock() {
            drv.write_block(block_id, buf)
        } else {
            Err(DriverError::Unsupported)
        }
    }

    fn block_size(&self) -> usize {
        if let Some(ref drv) = *NVME_DRIVER.lock() {
            drv.block_size()
        } else {
            512
        }
    }
}

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
        if let Some(nvme) = super::NvmeDriver::find_and_init() {
            *NVME_DRIVER.lock() = Some(nvme);
            let device_ref: Arc<Spinlock<Box<dyn Device>>> =
                Arc::new(Spinlock::new(Box::new(NvmeDeviceRef)));
            crate::device::DEVICE_MANAGER.write().register(device_ref);
            log::info!("[NVMe Module] Probed and registered NVMe Controller to DEVICE_MANAGER");
            Ok(())
        } else {
            Err(DriverError::InitFailed)
        }
    }
}
