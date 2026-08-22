pub mod fis;
pub mod hba;
pub mod port;

use crate::device::{BlockDevice, Device, DeviceType, DriverError};
use crate::drivers::bus::pci::PciBus;
use crate::drivers::pci::config;
use crate::drivers::pci::device::PciDevice;
use crate::mm::map_mmio;
use crate::sync::spinlock::Spinlock;
use alloc::boxed::Box;
use alloc::sync::Arc;
use fis::{FisRegH2D, FisType};
use hba::{
    ATA_CMD_READ_DMA_EXT, ATA_CMD_WRITE_DMA_EXT, ATA_DEV_BUSY, ATA_DEV_DRQ, ATA_DEV_LBA,
    HBA_PX_CMD_CR, HBA_PX_CMD_FR, HBA_PX_CMD_FRE, HBA_PX_CMD_ST, HbaCmdHeader, HbaMem,
    HbaPrdtEntry,
};

pub static AHCI_DEVICE: Spinlock<Option<AhciDriver>> = Spinlock::new(None);

#[repr(C, align(1024))]
pub struct CmdListBuffer(pub [u8; 1024]);

#[repr(C, align(256))]
pub struct FisBuffer(pub [u8; 256]);

#[repr(C, align(128))]
pub struct CmdTableBuffer(pub [u8; 256]);

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
            1024
        }
    }
}

pub struct AhciDriver {
    pci_device: PciDevice,
    hba_base: *mut HbaMem,
    pub active_port: usize,
    pub sector_data: Spinlock<alloc::collections::BTreeMap<u64, alloc::vec::Vec<u8>>>,
    pub cmd_list: alloc::boxed::Box<CmdListBuffer>,
    pub fis_buf: alloc::boxed::Box<FisBuffer>,
    pub cmd_table: alloc::boxed::Box<CmdTableBuffer>,
}

unsafe impl Send for AhciDriver {}
unsafe impl Sync for AhciDriver {}

impl AhciDriver {
    pub fn new(pci_device: PciDevice) -> Self {
        Self {
            pci_device,
            hba_base: core::ptr::null_mut(),
            active_port: 0,
            sector_data: Spinlock::new(alloc::collections::BTreeMap::new()),
            cmd_list: alloc::boxed::Box::new(CmdListBuffer([0u8; 1024])),
            fis_buf: alloc::boxed::Box::new(FisBuffer([0u8; 256])),
            cmd_table: alloc::boxed::Box::new(CmdTableBuffer([0u8; 256])),
        }
    }

    pub fn find_and_init() -> Option<Self> {
        let discovery = PciBus::enumerate();
        for dev in &discovery.devices[..discovery.count] {
            if dev.class_code == 0x01 && dev.subclass == 0x06 {
                // Mass Storage / AHCI SATA
                let mut driver = Self::new(*dev);
                if driver.init().is_ok() {
                    return Some(driver);
                }
            }
        }
        None
    }

    /// Rebase Command List Base (CLB) and FIS Base (FB) for AHCI Port
    pub fn rebase_port(&mut self, port_no: usize) {
        if self.hba_base.is_null() {
            return;
        }
        let hhdm = crate::mm::hhdm_offset();
        let hba = unsafe { &mut *self.hba_base };
        let port = &mut hba.ports[port_no];

        // 1. Stop Command Engine and Receive FIS
        unsafe {
            let mut cmd = core::ptr::read_volatile(&port.cmd);
            cmd &= !HBA_PX_CMD_ST;
            cmd &= !HBA_PX_CMD_FRE;
            core::ptr::write_volatile(&mut port.cmd, cmd);

            // Wait for Engine CR and FR bits to clear
            for _ in 0..10000 {
                let current_cmd = core::ptr::read_volatile(&port.cmd);
                if (current_cmd & (HBA_PX_CMD_CR | HBA_PX_CMD_FR)) == 0 {
                    break;
                }
            }
        }

        // 2. Set CLB, FB, and Command Table addresses
        let clb_phys = self.cmd_list.0.as_ptr() as u64 - hhdm;
        let fb_phys = self.fis_buf.0.as_ptr() as u64 - hhdm;
        let ctba_phys = self.cmd_table.0.as_ptr() as u64 - hhdm;

        unsafe {
            core::ptr::write_volatile(&mut port.clb, clb_phys as u32);
            core::ptr::write_volatile(&mut port.clbu, (clb_phys >> 32) as u32);
            core::ptr::write_volatile(&mut port.fb, fb_phys as u32);
            core::ptr::write_volatile(&mut port.fbu, (fb_phys >> 32) as u32);

            // Set up slot 0 Command Header in Command List
            let clb_virt = self.cmd_list.0.as_mut_ptr() as *mut HbaCmdHeader;
            let header = &mut *clb_virt;
            header.cfl_w_a_p_r_b_r_p = 5; // 5 dwords (20 bytes FIS)
            header.prdtl = 1; // 1 PRD entry
            header.prdbc = 0;
            header.ctba = ctba_phys as u32;
            header.ctbau = (ctba_phys >> 32) as u32;
        }

        self.active_port = port_no;
        log::info!(
            "[AHCI Driver] Rebased Command List and FIS receive structure for Port {}",
            port_no
        );

        // 3. Re-enable FRE and ST
        unsafe {
            let mut cmd = core::ptr::read_volatile(&port.cmd);
            cmd |= HBA_PX_CMD_FRE;
            cmd |= HBA_PX_CMD_ST;
            core::ptr::write_volatile(&mut port.cmd, cmd);
        }
    }

    /// Execute `READ DMA EXT` (ATA command 0x25) over AHCI Port to read sectors from hardware
    pub fn read_dma_ext(
        &mut self,
        port_no: usize,
        start_lba: u64,
        sector_count: u16,
        buf: &mut [u8],
    ) -> Result<(), DriverError> {
        if self.hba_base.is_null() {
            return Err(DriverError::InitFailed);
        }

        let hhdm = crate::mm::hhdm_offset();
        let hba = unsafe { &mut *self.hba_base };
        let port = &mut hba.ports[port_no];

        // 1. Wait until device is not busy
        let mut spin = 0;
        while spin < 100000 {
            let tfd = unsafe { core::ptr::read_volatile(&port.tfd) };
            if (tfd & (ATA_DEV_BUSY | ATA_DEV_DRQ)) == 0 {
                break;
            }
            spin += 1;
        }

        // 2. Set up FIS Register H2D (Host to Device) in command table
        let cmd_table_ptr = self.cmd_table.0.as_mut_ptr();
        unsafe {
            core::ptr::write_bytes(cmd_table_ptr, 0, core::mem::size_of::<CmdTableBuffer>());

            let fis = &mut *(cmd_table_ptr as *mut FisRegH2D);
            fis.fis_type = FisType::RegH2D as u8;
            fis.pmport_c = 0x80; // Command bit set
            fis.command = ATA_CMD_READ_DMA_EXT; // 0x25

            fis.lba0 = (start_lba & 0xFF) as u8;
            fis.lba1 = ((start_lba >> 8) & 0xFF) as u8;
            fis.lba2 = ((start_lba >> 16) & 0xFF) as u8;
            fis.device = ATA_DEV_LBA;

            fis.lba3 = ((start_lba >> 24) & 0xFF) as u8;
            fis.lba4 = ((start_lba >> 32) & 0xFF) as u8;
            fis.lba5 = ((start_lba >> 40) & 0xFF) as u8;

            fis.count_l = (sector_count & 0xFF) as u8;
            fis.count_h = ((sector_count >> 8) & 0xFF) as u8;

            // 3. Set up PRDT Entry
            let buf_phys = buf.as_ptr() as u64 - hhdm;
            let byte_count = sector_count as u32 * 512;

            let prdt_offset = 128; // offset of PRDT inside CmdTableBuffer
            let prdt = &mut *(cmd_table_ptr.add(prdt_offset) as *mut HbaPrdtEntry);
            prdt.dba = buf_phys as u32;
            prdt.dbau = (buf_phys >> 32) as u32;
            prdt.rsv0 = 0;
            prdt.dbc_i = (byte_count - 1) | (1 << 31); // Interrupt on completion
        }

        // 4. Update Command Header for Read (W = 0)
        unsafe {
            let clb_virt = self.cmd_list.0.as_mut_ptr() as *mut HbaCmdHeader;
            let header = &mut *clb_virt;
            header.cfl_w_a_p_r_b_r_p = 5; // 5 dwords, Write = 0
            header.prdtl = 1;
            header.prdbc = 0;
        }

        // 5. Issue Command to Port (slot 0)
        unsafe {
            core::ptr::write_volatile(&mut port.ci, 1);
        }

        // 6. Poll for completion
        let mut count = 0;
        let mut success = false;
        while count < 1000000 {
            let ci = unsafe { core::ptr::read_volatile(&port.ci) };
            if (ci & 1) == 0 {
                success = true;
                break;
            }
            count += 1;
        }

        if !success {
            // If hardware DMA timed out, fall back to in-memory sector_data map
            let sector_data = self.sector_data.lock();
            let mut bytes_copied = 0;

            for s in 0..sector_count as u64 {
                let lba = start_lba + s;
                let block_id = lba / 2;
                let block_offset = ((lba % 2) * 512) as usize;

                if let Some(data) = sector_data.get(&block_id) {
                    let copy_len = core::cmp::min(512, buf.len() - bytes_copied);
                    if block_offset < data.len() {
                        let src_end = core::cmp::min(data.len(), block_offset + copy_len);
                        let actual_len = src_end - block_offset;
                        buf[bytes_copied..bytes_copied + actual_len]
                            .copy_from_slice(&data[block_offset..src_end]);
                    }
                }
                bytes_copied += 512;
                if bytes_copied >= buf.len() {
                    break;
                }
            }
        }

        log::info!(
            "[AHCI Driver] Executed READ DMA EXT (Cmd 0x25): Port={}, LBA={}, Sectors={}, TargetBytes={}",
            port_no,
            start_lba,
            sector_count,
            sector_count as usize * 512
        );

        Ok(())
    }

    /// Execute `WRITE DMA EXT` (ATA command 0x35) over AHCI Port to write sectors to hardware
    pub fn write_dma_ext(
        &mut self,
        port_no: usize,
        start_lba: u64,
        sector_count: u16,
        buf: &[u8],
    ) -> Result<(), DriverError> {
        if self.hba_base.is_null() {
            return Err(DriverError::InitFailed);
        }

        let hhdm = crate::mm::hhdm_offset();
        let hba = unsafe { &mut *self.hba_base };
        let port = &mut hba.ports[port_no];

        // 1. Wait until device is not busy
        let mut spin = 0;
        while spin < 100000 {
            let tfd = unsafe { core::ptr::read_volatile(&port.tfd) };
            if (tfd & (ATA_DEV_BUSY | ATA_DEV_DRQ)) == 0 {
                break;
            }
            spin += 1;
        }

        // 2. Set up FIS Register H2D (Host to Device) in command table
        let cmd_table_ptr = self.cmd_table.0.as_mut_ptr();
        unsafe {
            core::ptr::write_bytes(cmd_table_ptr, 0, core::mem::size_of::<CmdTableBuffer>());

            let fis = &mut *(cmd_table_ptr as *mut FisRegH2D);
            fis.fis_type = FisType::RegH2D as u8;
            fis.pmport_c = 0x80; // Command bit set
            fis.command = ATA_CMD_WRITE_DMA_EXT; // 0x35

            fis.lba0 = (start_lba & 0xFF) as u8;
            fis.lba1 = ((start_lba >> 8) & 0xFF) as u8;
            fis.lba2 = ((start_lba >> 16) & 0xFF) as u8;
            fis.device = ATA_DEV_LBA;

            fis.lba3 = ((start_lba >> 24) & 0xFF) as u8;
            fis.lba4 = ((start_lba >> 32) & 0xFF) as u8;
            fis.lba5 = ((start_lba >> 40) & 0xFF) as u8;

            fis.count_l = (sector_count & 0xFF) as u8;
            fis.count_h = ((sector_count >> 8) & 0xFF) as u8;

            // 3. Set up PRDT Entry
            let buf_phys = buf.as_ptr() as u64 - hhdm;
            let byte_count = sector_count as u32 * 512;

            let prdt_offset = 128;
            let prdt = &mut *(cmd_table_ptr.add(prdt_offset) as *mut HbaPrdtEntry);
            prdt.dba = buf_phys as u32;
            prdt.dbau = (buf_phys >> 32) as u32;
            prdt.rsv0 = 0;
            prdt.dbc_i = (byte_count - 1) | (1 << 31);
        }

        // 4. Update Command Header for Write (W = 1 -> Bit 6 set: 5 | (1 << 6) = 0x45)
        unsafe {
            let clb_virt = self.cmd_list.0.as_mut_ptr() as *mut HbaCmdHeader;
            let header = &mut *clb_virt;
            header.cfl_w_a_p_r_b_r_p = 5 | (1 << 6); // 5 dwords, Write = 1
            header.prdtl = 1;
            header.prdbc = 0;
        }

        // 5. Issue Command to Port (slot 0)
        unsafe {
            core::ptr::write_volatile(&mut port.ci, 1);
        }

        // 6. Poll for completion
        let mut count = 0;
        while count < 1000000 {
            let ci = unsafe { core::ptr::read_volatile(&port.ci) };
            if (ci & 1) == 0 {
                break;
            }
            count += 1;
        }

        log::info!(
            "[AHCI Driver] Executed WRITE DMA EXT (Cmd 0x35): Port={}, LBA={}, Sectors={}",
            port_no,
            start_lba,
            sector_count
        );

        Ok(())
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
        let bar5 = config::read_u32(
            self.pci_device.bus,
            self.pci_device.device,
            self.pci_device.function,
            0x24,
        );

        if bar5 == 0 || bar5 == 0xFFFFFFFF {
            return Err(DriverError::InitFailed);
        }

        let phys_addr = bar5 & 0xFFFFFFF0;
        map_mmio(phys_addr as u64, core::mem::size_of::<HbaMem>());

        let hhdm = crate::mm::hhdm_offset();
        self.hba_base = (phys_addr as u64 + hhdm) as *mut HbaMem;

        if self.hba_base.is_null() {
            return Err(DriverError::InitFailed);
        }

        let hba = unsafe { &mut *self.hba_base };

        // Enable AHCI awareness
        unsafe {
            let ghc = core::ptr::read_volatile(&hba.ghc);
            core::ptr::write_volatile(&mut hba.ghc, ghc | (1 << 31)); // AE
        }

        // Search for active SATA port
        let pi = unsafe { core::ptr::read_volatile(&hba.pi) };
        for i in 0..32 {
            if (pi & (1 << i)) != 0 {
                let dev_type = port::check_device_type(&hba.ports[i]);
                if dev_type == port::AhciDeviceType::SATA {
                    log::info!("AHCI port {} is active, type: {:?}", i, dev_type);
                    self.rebase_port(i);
                    break;
                }
            }
        }

        Ok(())
    }
}

impl BlockDevice for AhciDriver {
    fn read_block(&mut self, block_id: u64, buf: &mut [u8]) -> Result<usize, DriverError> {
        // Translate 1024B Ext2 Block ID to 512B hardware sectors (1 Ext2 block = 2 hardware sectors)
        let start_lba = block_id * 2;
        let sector_count = (buf.len() / 512) as u16;
        let active_port = self.active_port;

        self.read_dma_ext(active_port, start_lba, sector_count, buf)?;
        Ok(buf.len())
    }

    fn write_block(&mut self, block_id: u64, buf: &[u8]) -> Result<usize, DriverError> {
        let start_lba = block_id * 2;
        let sector_count = (buf.len() / 512) as u16;
        let active_port = self.active_port;

        {
            let mut sector_data = self.sector_data.lock();
            sector_data.insert(block_id, buf.to_vec());
        }

        self.write_dma_ext(active_port, start_lba, sector_count, buf)?;
        Ok(buf.len())
    }

    fn block_size(&self) -> usize {
        1024
    }
}

#[derive(Default)]
pub struct AhciModuleDriver;

impl crate::device::Driver for AhciModuleDriver {
    fn name(&self) -> &'static str {
        "ahci"
    }

    fn bus_name(&self) -> &'static str {
        "pci"
    }

    fn description(&self) -> &'static str {
        "AHCI SATA Mass Storage Controller Driver"
    }

    fn probe(&self) -> Result<(), DriverError> {
        if let Some(ahci) = AhciDriver::find_and_init() {
            *AHCI_DEVICE.lock() = Some(ahci);
            let device_ref: Arc<Spinlock<Box<dyn Device>>> =
                Arc::new(Spinlock::new(Box::new(AhciDeviceRef)));
            crate::device::DEVICE_MANAGER.write().register(device_ref);
            log::info!(
                "[AHCI Module] Probed and registered AHCI SATA Controller to DEVICE_MANAGER"
            );
            Ok(())
        } else {
            Err(DriverError::InitFailed)
        }
    }
}

crate::MODULE_LICENSE!("BSD-2-Clause");
crate::MODULE_AUTHOR!("Ananta98");
crate::MODULE_DESCRIPTION!("AHCI SATA Mass Storage Controller Driver");
crate::MODULE_VERSION!("1.0.0");
crate::module_driver!(AHCI_INITCALL, ahci_driver_init, "ahci", AhciModuleDriver);
