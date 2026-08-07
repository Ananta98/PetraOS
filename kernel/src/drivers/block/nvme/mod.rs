pub mod queue;
pub mod regs;

use crate::arch::paging;
use crate::drivers::pci::config;
use crate::drivers::pci::device::PciDevice;
use crate::drivers::{BlockDevice, Device, DeviceType, DriverError};
use crate::sync::spinlock::Spinlock;
use queue::NvmeQueue;
use regs::NvmeRegs;

pub static NVME_DEVICE: Spinlock<Option<NvmeDriver>> = Spinlock::new(None);

pub struct NvmeDeviceRef;

impl Device for NvmeDeviceRef {
    fn dev_type(&self) -> DeviceType {
        DeviceType::Block
    }

    fn name(&self) -> &'static str {
        "NVMe Controller"
    }

    fn init(&mut self) -> Result<(), DriverError> {
        if let Some(ref mut drv) = *NVME_DEVICE.lock() {
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
        if let Some(ref mut drv) = *NVME_DEVICE.lock() {
            drv.read_block(block_id, buf)
        } else {
            Err(DriverError::Unsupported)
        }
    }

    fn write_block(&mut self, block_id: u64, buf: &[u8]) -> Result<usize, DriverError> {
        if let Some(ref mut drv) = *NVME_DEVICE.lock() {
            drv.write_block(block_id, buf)
        } else {
            Err(DriverError::Unsupported)
        }
    }

    fn block_size(&self) -> usize {
        if let Some(ref drv) = *NVME_DEVICE.lock() {
            drv.block_size()
        } else {
            4096
        }
    }
}

pub struct NvmeDriver {
    pci_device: PciDevice,
    regs: *mut NvmeRegs,
    admin_queue: Option<NvmeQueue>,
}

unsafe impl Send for NvmeDriver {}
unsafe impl Sync for NvmeDriver {}

impl NvmeDriver {
    pub const fn new(pci_device: PciDevice) -> Self {
        Self {
            pci_device,
            regs: core::ptr::null_mut(),
            admin_queue: None,
        }
    }

    pub fn find_and_init() -> Option<Self> {
        let discovery = crate::drivers::pci::bus::PciBus::enumerate();
        for dev in &discovery.devices[..discovery.count] {
            if dev.class_code == 0x01 && dev.subclass == 0x08 {
                // Mass Storage / NVMe
                let mut driver = Self::new(*dev);
                if driver.init().is_ok() {
                    return Some(driver);
                }
            }
        }
        None
    }
}

impl Device for NvmeDriver {
    fn dev_type(&self) -> DeviceType {
        DeviceType::Block
    }

    fn name(&self) -> &'static str {
        "NVMe Controller"
    }

    fn init(&mut self) -> Result<(), DriverError> {
        // Read BAR0 (offset 0x10) from PCI config space
        let bar0 = config::read_u32(
            self.pci_device.bus,
            self.pci_device.device,
            self.pci_device.function,
            0x10, // BAR0 offset
        );

        if bar0 == 0 || bar0 == 0xFFFFFFFF {
            return Err(DriverError::InitFailed);
        }

        // Mask off type bits to get physical address
        let phys_addr = bar0 & 0xFFFFFFF0;

        paging::map_mmio(phys_addr as u64, 16384);

        let hhdm = crate::mm::hhdm_offset();
        self.regs = (phys_addr as u64 + hhdm) as *mut NvmeRegs;

        if self.regs.is_null() {
            return Err(DriverError::InitFailed);
        }

        let regs = unsafe { &mut *self.regs };

        // 1. Disable controller first to configure it
        unsafe {
            let mut cc = core::ptr::read_volatile(&regs.cc);
            cc &= !1; // Clear EN (Enable) bit
            core::ptr::write_volatile(&mut regs.cc, cc);
        }

        // Wait for ready (CSTS.RDY) to become 0
        let mut timeout = 100000;
        while timeout > 0 {
            let csts = unsafe { core::ptr::read_volatile(&regs.csts) };
            if (csts & 1) == 0 {
                break;
            }
            timeout -= 1;
        }
        if timeout == 0 {
            return Err(DriverError::InitFailed);
        }

        // Read Capabilities and Version
        let cap = unsafe { core::ptr::read_volatile(&regs.cap) };
        let vs = unsafe { core::ptr::read_volatile(&regs.vs) };
        let major = (vs >> 16) & 0xFFFF;
        let minor = (vs >> 8) & 0xFF;
        log::info!(
            "NVMe Version: {}.{}, Capabilities: {:#x}",
            major,
            minor,
            cap
        );

        // 2. Configure Admin Queue Attributes (AQA)
        // Set admin submission/completion queue sizes (e.g., 64 entries each)
        // AQA is [completion queue size - 1 | submission queue size - 1]
        let aqa = (63 << 16) | 63;
        unsafe {
            core::ptr::write_volatile(&mut regs.aqa, aqa);
        }

        // 3. Enable controller
        unsafe {
            let mut cc = core::ptr::read_volatile(&regs.cc);
            cc |= 1; // Set EN (Enable) bit
            cc |= 0 << 16; // IOCQES: I/O Completion Queue Entry Size (2^4 = 16 bytes)
            cc |= 6 << 20; // IOSQES: I/O Submission Queue Entry Size (2^6 = 64 bytes)
            core::ptr::write_volatile(&mut regs.cc, cc);
        }

        // Wait for ready (CSTS.RDY) to become 1
        timeout = 100000;
        while timeout > 0 {
            let csts = unsafe { core::ptr::read_volatile(&regs.csts) };
            if (csts & 1) != 0 {
                break;
            }
            timeout -= 1;
        }
        if timeout == 0 {
            return Err(DriverError::InitFailed);
        }

        log::info!("NVMe Controller initialized successfully");
        Ok(())
    }
}

impl BlockDevice for NvmeDriver {
    fn read_block(&mut self, _block_id: u64, _buf: &mut [u8]) -> Result<usize, DriverError> {
        Err(DriverError::Unsupported)
    }

    fn write_block(&mut self, _block_id: u64, _buf: &[u8]) -> Result<usize, DriverError> {
        Err(DriverError::Unsupported)
    }

    fn block_size(&self) -> usize {
        4096
    }
}
