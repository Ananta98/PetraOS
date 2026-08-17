pub mod command;
pub mod device;
pub mod queue;
pub mod regs;

use crate::device::{BlockDevice, Device, DeviceType, DriverError};
use crate::drivers::pci::config;
use crate::drivers::pci::device::PciDevice;
use crate::mm::pmm::PMM;
use crate::mm::{ensure_mapped, map_mmio};
use crate::sync::spinlock::Spinlock;
use x86_64::PhysAddr;

pub use command::{NvmeCmdBuilder, NvmeIdentifyNamespace};
pub use device::{NvmeDeviceRef, NvmeModuleDriver};
pub use queue::{NvmeCmd, NvmeCqe, NvmeQueue};
pub use regs::{
    NVME_CC_CSS_NVM, NVME_CC_EN, NVME_CC_IOCQES_16, NVME_CC_IOSQES_64, NVME_CC_MPS_4K,
    NVME_CSTS_RDY, NvmeRegs,
};

pub static NVME_DRIVER: Spinlock<Option<NvmeDriver>> = Spinlock::new(None);

/// Helper to allocate a physical DMA page and ensure it is mapped in the kernel page table.
fn alloc_dma_page() -> Result<(PhysAddr, *mut u8), DriverError> {
    let phys = PMM.alloc_page().ok_or(DriverError::InitFailed)?;
    ensure_mapped(phys.as_u64(), 4096);
    let hhdm = crate::mm::hhdm_offset();
    let virt = (phys.as_u64() + hhdm) as *mut u8;
    Ok((phys, virt))
}

/// Helper to free an allocated DMA physical page.
fn free_dma_page(phys: PhysAddr) {
    PMM.free_page(phys);
}

pub struct NvmeDriver {
    pci_device: PciDevice,
    regs: *mut NvmeRegs,
    dstrd: u32,
    admin_queue: Option<NvmeQueue>,
    io_queue: Option<NvmeQueue>,
    block_size: usize,
    sector_count: u64,
    cid_counter: u16,
}

unsafe impl Send for NvmeDriver {}
unsafe impl Sync for NvmeDriver {}

impl NvmeDriver {
    pub const fn new(pci_device: PciDevice) -> Self {
        Self {
            pci_device,
            regs: core::ptr::null_mut(),
            dstrd: 0,
            admin_queue: None,
            io_queue: None,
            block_size: 512,
            sector_count: 0,
            cid_counter: 1,
        }
    }

    fn next_cid(&mut self) -> u16 {
        let cid = self.cid_counter;
        self.cid_counter = self.cid_counter.wrapping_add(1);
        if self.cid_counter == 0 {
            self.cid_counter = 1;
        }
        cid
    }

    pub fn find_and_init() -> Option<Self> {
        let discovery = crate::drivers::pci::bus::PciBus::enumerate();
        for dev in &discovery.devices[..discovery.count] {
            if dev.class_code == 0x01 && dev.subclass == 0x08 {
                // Mass Storage Controller / NVMe Subclass
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
        // 1. Enable PCI Memory Space Access (bit 1) and Bus Master Enable (bit 2)
        let pci_cmd = config::read_u16(
            self.pci_device.bus,
            self.pci_device.device,
            self.pci_device.function,
            0x04, // PCI Command Register
        );
        config::write_u16(
            self.pci_device.bus,
            self.pci_device.device,
            self.pci_device.function,
            0x04,
            pci_cmd | 0x06, // Enable Memory Space & Bus Master
        );

        // 2. Read PCI BAR0 (0x10) and BAR1 (0x14)
        let bar0 = config::read_u32(
            self.pci_device.bus,
            self.pci_device.device,
            self.pci_device.function,
            0x10,
        );

        if bar0 == 0 || bar0 == 0xFFFFFFFF {
            return Err(DriverError::InitFailed);
        }

        let is_64bit = (bar0 & 0x06) == 0x04;
        let bar1 = if is_64bit {
            config::read_u32(
                self.pci_device.bus,
                self.pci_device.device,
                self.pci_device.function,
                0x14,
            )
        } else {
            0
        };

        let phys_addr = if is_64bit {
            ((bar1 as u64) << 32) | ((bar0 & !0x0F) as u64)
        } else {
            (bar0 & !0x0F) as u64
        };

        log::info!(
            "NVMe PCI BAR0: {:#x}, BAR1: {:#x}, 64bit: {}, phys_addr: {:#x}",
            bar0,
            bar1,
            is_64bit,
            phys_addr
        );

        // 3. Map MMIO registers
        map_mmio(phys_addr, 16384);
        let hhdm = crate::mm::hhdm_offset();
        self.regs = (phys_addr + hhdm) as *mut NvmeRegs;

        log::info!("NVMe mapped MMIO virtual address: {:p}", self.regs);

        if self.regs.is_null() {
            return Err(DriverError::InitFailed);
        }

        let regs = unsafe { &mut *self.regs };

        // 4. Disable Controller to reset state
        unsafe {
            let mut cc = core::ptr::read_volatile(&regs.cc);
            cc &= !NVME_CC_EN;
            core::ptr::write_volatile(&mut regs.cc, cc);
        }

        // Poll CSTS.RDY until 0
        let mut timeout = 100_000usize;
        while timeout > 0 {
            let csts = unsafe { core::ptr::read_volatile(&regs.csts) };
            if (csts & NVME_CSTS_RDY) == 0 {
                break;
            }
            timeout -= 1;
        }
        if timeout == 0 {
            log::error!("NVMe: Controller disable timeout (CSTS.RDY stuck at 1)");
            return Err(DriverError::InitFailed);
        }

        // 5. Read Capabilities
        let cap = unsafe { core::ptr::read_volatile(&regs.cap) };
        self.dstrd = ((cap >> 32) & 0x0F) as u32;

        // 6. Allocate DMA memory for Admin Submission Queue (ASQ) and Completion Queue (ACQ)
        let (asq_phys_addr, asq_virt_u8) = alloc_dma_page()?;
        let (acq_phys_addr, acq_virt_u8) = alloc_dma_page()?;

        let asq_virt = asq_virt_u8 as *mut NvmeCmd;
        let acq_virt = acq_virt_u8 as *mut NvmeCqe;

        // SAFETY: PMM returned valid page physical addresses, mapped via HHDM and ensure_mapped.
        unsafe {
            core::ptr::write_bytes(asq_virt_u8, 0, 4096);
            core::ptr::write_bytes(acq_virt_u8, 0, 4096);
        }

        // Set AQA (64 entries each: 63 | (63 << 16))
        let aqa = (63u32 << 16) | 63u32;
        unsafe {
            core::ptr::write_volatile(&mut regs.aqa, aqa);
            core::ptr::write_volatile(&mut regs.asq, asq_phys_addr.as_u64());
            core::ptr::write_volatile(&mut regs.acq, acq_phys_addr.as_u64());
        }

        let asq_db = NvmeRegs::doorbell_ptr(self.regs, 0, false, self.dstrd);
        let acq_db = NvmeRegs::doorbell_ptr(self.regs, 0, true, self.dstrd);

        self.admin_queue = Some(NvmeQueue::new(
            0,
            64,
            asq_virt,
            asq_phys_addr.as_u64(),
            acq_virt,
            acq_phys_addr.as_u64(),
            asq_db,
            acq_db,
        ));

        // 7. Enable Controller
        unsafe {
            let cc = NVME_CC_EN
                | NVME_CC_CSS_NVM
                | NVME_CC_MPS_4K
                | NVME_CC_IOSQES_64
                | NVME_CC_IOCQES_16;
            core::ptr::write_volatile(&mut regs.cc, cc);
        }

        // Wait for CSTS.RDY to become 1
        timeout = 100_000usize;
        while timeout > 0 {
            let csts = unsafe { core::ptr::read_volatile(&regs.csts) };
            if (csts & NVME_CSTS_RDY) != 0 {
                break;
            }
            timeout -= 1;
        }
        if timeout == 0 {
            log::error!("NVMe: Controller enable timeout (CSTS.RDY stuck at 0)");
            return Err(DriverError::InitFailed);
        }

        // 8. Setup I/O Submission & Completion Queues (QID 1)
        let (iosq_phys, iosq_virt_u8) = alloc_dma_page()?;
        let (iocq_phys, iocq_virt_u8) = alloc_dma_page()?;

        let iosq_virt = iosq_virt_u8 as *mut NvmeCmd;
        let iocq_virt = iocq_virt_u8 as *mut NvmeCqe;

        // SAFETY: PMM returned valid page physical addresses, mapped via HHDM and ensure_mapped.
        unsafe {
            core::ptr::write_bytes(iosq_virt_u8, 0, 4096);
            core::ptr::write_bytes(iocq_virt_u8, 0, 4096);
        }

        let create_cq_cmd = NvmeCmdBuilder::create_cq(self.next_cid(), 1, 64, iocq_phys.as_u64());
        if let Some(ref mut admin_q) = self.admin_queue {
            admin_q.submit_and_wait(create_cq_cmd)?;
        }

        let create_sq_cmd =
            NvmeCmdBuilder::create_sq(self.next_cid(), 1, 1, 64, iosq_phys.as_u64());
        if let Some(ref mut admin_q) = self.admin_queue {
            admin_q.submit_and_wait(create_sq_cmd)?;
        }

        let iosq_db = NvmeRegs::doorbell_ptr(self.regs, 1, false, self.dstrd);
        let iocq_db = NvmeRegs::doorbell_ptr(self.regs, 1, true, self.dstrd);

        self.io_queue = Some(NvmeQueue::new(
            1,
            64,
            iosq_virt,
            iosq_phys.as_u64(),
            iocq_virt,
            iocq_phys.as_u64(),
            iosq_db,
            iocq_db,
        ));

        // 9. Identify Namespace 1 to retrieve capacity and sector size
        let (id_phys, id_virt) = alloc_dma_page()?;

        // SAFETY: Zeroing the 4096-byte page allocated for Identify Namespace response.
        unsafe {
            core::ptr::write_bytes(id_virt, 0, 4096);
        }

        let identify_cmd = NvmeCmdBuilder::identify_ns(self.next_cid(), 1, id_phys.as_u64());
        if let Some(ref mut admin_q) = self.admin_queue {
            admin_q.submit_and_wait(identify_cmd)?;
        }

        let id_ns = unsafe { &*(id_virt as *const NvmeIdentifyNamespace) };
        self.block_size = id_ns.block_size();
        self.sector_count = id_ns.nsze;

        free_dma_page(id_phys);

        let capacity_mb = (self.sector_count * self.block_size as u64) / (1024 * 1024);
        log::info!(
            "NVMe Controller initialized: SectorSize={} B, Sectors={}, Capacity={} MB",
            self.block_size,
            self.sector_count,
            capacity_mb
        );

        Ok(())
    }
}

impl BlockDevice for NvmeDriver {
    fn read_block(&mut self, block_id: u64, buf: &mut [u8]) -> Result<usize, DriverError> {
        if buf.is_empty() {
            return Ok(0);
        }

        let block_count = (buf.len() + self.block_size - 1) / self.block_size;
        if block_count > 8 {
            // Cap single transfer size to 1 page (4096 bytes)
            return Err(DriverError::Unsupported);
        }

        let (dma_phys, dma_virt) = alloc_dma_page().map_err(|_| DriverError::ReadFailed)?;

        let cid = self.next_cid();
        let read_cmd =
            NvmeCmdBuilder::read(cid, 1, block_id, block_count as u16, dma_phys.as_u64());

        let result = if let Some(ref mut io_q) = self.io_queue {
            io_q.submit_and_wait(read_cmd)
        } else {
            Err(DriverError::InitFailed)
        };

        if result.is_ok() {
            unsafe {
                // SAFETY: Copying read bytes from allocated DMA page to caller's buffer.
                core::ptr::copy_nonoverlapping(dma_virt, buf.as_mut_ptr(), buf.len());
            }
        }

        free_dma_page(dma_phys);
        result.map(|_| buf.len())
    }

    fn write_block(&mut self, block_id: u64, buf: &[u8]) -> Result<usize, DriverError> {
        if buf.is_empty() {
            return Ok(0);
        }

        let block_count = (buf.len() + self.block_size - 1) / self.block_size;
        if block_count > 8 {
            // Cap single transfer size to 1 page (4096 bytes)
            return Err(DriverError::Unsupported);
        }

        let (dma_phys, dma_virt) = alloc_dma_page().map_err(|_| DriverError::WriteFailed)?;

        unsafe {
            // SAFETY: Copying data from caller's buffer to allocated DMA page prior to transfer.
            core::ptr::copy_nonoverlapping(buf.as_ptr(), dma_virt, buf.len());
        }

        let cid = self.next_cid();
        let write_cmd =
            NvmeCmdBuilder::write(cid, 1, block_id, block_count as u16, dma_phys.as_u64());

        let result = if let Some(ref mut io_q) = self.io_queue {
            io_q.submit_and_wait(write_cmd)
        } else {
            Err(DriverError::InitFailed)
        };

        free_dma_page(dma_phys);
        result.map(|_| buf.len())
    }

    fn block_size(&self) -> usize {
        self.block_size
    }
}

crate::MODULE_LICENSE!("BSD-2-Clause");
crate::MODULE_AUTHOR!("PetraOS Development Team");
crate::MODULE_DESCRIPTION!("NVM Express Block Device Driver");
crate::MODULE_VERSION!("1.0.0");

crate::module_driver!(NVME_INITCALL, nvme_driver_init, "nvme", NvmeModuleDriver);
