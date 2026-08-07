pub mod fis;
pub mod hba;
pub mod port;

use crate::arch::paging;
use crate::drivers::pci::config;
use crate::drivers::pci::device::PciDevice;
use crate::drivers::{BlockDevice, Device, DeviceType, DriverError};
use crate::sync::spinlock::Spinlock;
use hba::HbaMem;

pub static AHCI_DEVICE: Spinlock<Option<AhciDriver>> = Spinlock::new(None);

pub struct AhciDeviceRef;

impl Device for AhciDeviceRef {
    fn dev_type(&self) -> DeviceType {
        DeviceType::Block
    }

    fn name(&self) -> &'static str {
        "AHCI SATA Controller"
    }

    fn init(&mut self) -> Result<(), DriverError> {
        if let Some(ref mut drv) = *AHCI_DEVICE.lock() {
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

impl BlockDevice for AhciDeviceRef {
    fn read_block(&mut self, block_id: u64, buf: &mut [u8]) -> Result<usize, DriverError> {
        if let Some(ref mut drv) = *AHCI_DEVICE.lock() {
            drv.read_block(block_id, buf)
        } else {
            Err(DriverError::Unsupported)
        }
    }

    fn write_block(&mut self, block_id: u64, buf: &[u8]) -> Result<usize, DriverError> {
        if let Some(ref mut drv) = *AHCI_DEVICE.lock() {
            drv.write_block(block_id, buf)
        } else {
            Err(DriverError::Unsupported)
        }
    }

    fn block_size(&self) -> usize {
        if let Some(ref drv) = *AHCI_DEVICE.lock() {
            drv.block_size()
        } else {
            512
        }
    }
}

pub struct AhciDriver {
    pci_device: PciDevice,
    hba_base: *mut HbaMem,
}

unsafe impl Send for AhciDriver {}
unsafe impl Sync for AhciDriver {}

impl AhciDriver {
    pub const fn new(pci_device: PciDevice) -> Self {
        Self {
            pci_device,
            hba_base: core::ptr::null_mut(),
        }
    }

    pub fn find_and_init() -> Option<Self> {
        let discovery = crate::drivers::pci::bus::PciBus::enumerate();
        for dev in &discovery.devices[..discovery.count] {
            if dev.class_code == 0x01 && dev.subclass == 0x06 {
                // Mass Storage / SATA
                let mut driver = Self::new(*dev);
                if driver.init().is_ok() {
                    return Some(driver);
                }
            }
        }
        None
    }
}

impl Device for AhciDriver {
    fn dev_type(&self) -> DeviceType {
        DeviceType::Block
    }

    fn name(&self) -> &'static str {
        "AHCI SATA Controller"
    }

    fn init(&mut self) -> Result<(), DriverError> {
        // Read BAR5 from PCI config space
        let bar5 = config::read_u32(
            self.pci_device.bus,
            self.pci_device.device,
            self.pci_device.function,
            0x24, // BAR5 offset
        );

        if bar5 == 0 || bar5 == 0xFFFFFFFF {
            return Err(DriverError::InitFailed);
        }

        // Mask off lower bits (BAR type bits) to get physical address
        let phys_addr = bar5 & 0xFFFFFFF0;

        paging::map_mmio(phys_addr as u64, core::mem::size_of::<HbaMem>());

        let hhdm = crate::mm::hhdm_offset();
        self.hba_base = (phys_addr as u64 + hhdm) as *mut HbaMem;

        if self.hba_base.is_null() {
            return Err(DriverError::InitFailed);
        }

        let hba = unsafe { &mut *self.hba_base };

        // Enable AHCI awareness
        unsafe {
            let ghc = core::ptr::read_volatile(&hba.ghc);
            core::ptr::write_volatile(&mut hba.ghc, ghc | (1 << 31)); // AE (AHCI Enable)
        }

        // Check ports
        let pi = unsafe { core::ptr::read_volatile(&hba.pi) };
        for i in 0..32 {
            if (pi & (1 << i)) != 0 {
                let dev_type = port::check_device_type(&hba.ports[i]);
                if dev_type != port::AhciDeviceType::None {
                    log::info!("AHCI port {} is active, type: {:?}", i, dev_type);
                }
            }
        }

        Ok(())
    }
}

impl BlockDevice for AhciDriver {
    fn read_block(&mut self, _block_id: u64, _buf: &mut [u8]) -> Result<usize, DriverError> {
        Err(DriverError::Unsupported)
    }

    fn write_block(&mut self, _block_id: u64, _buf: &[u8]) -> Result<usize, DriverError> {
        Err(DriverError::Unsupported)
    }

    fn block_size(&self) -> usize {
        512
    }
}
