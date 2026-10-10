// SPDX-License-Identifier: GPL-2.0

//! AHCI Port Management and SATA BlockDevice Implementation.
//!
//! Handles port initialization, command list/FIS configuration using `ostd::mm::dma::DmaCoherent`,
//! and ATA Read/Write operations via DMA.

use alloc::string::String;
use spin::Mutex;

use ostd::{
    io::IoMem,
    mm::{HasPaddr, VmIoOnce, VmReader, VmWriter, dma::DmaCoherent, io::util::HasVmReaderWriter},
};

use super::{
    fis::{FisRegH2D, FisType},
    hba::*,
};
use crate::{
    api::errno::{Errno, Result},
    drivers::{Device, DeviceType, block::BlockDevice},
};

/// Type of device attached to an AHCI port.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AhciDeviceType {
    None,
    Sata,
    Satapi,
    Semb,
    PortMultiplier,
    Unknown,
}

/// Represents an individual AHCI Port and attached SATA drive.
pub struct AhciPort {
    pub port_no: usize,
    pub port_base: usize,
    pub name: String,
    pub sector_count: u64,
    pub sector_size: usize,
    pub mmio: IoMem,
    // Coherent DMA buffers for Port:
    // Command List: 1024 bytes (32 headers * 32 bytes)
    pub cmd_list_dma: DmaCoherent,
    // FIS Receive Area: 256 bytes
    pub rx_fis_dma: DmaCoherent,
    // Command Table for Slot 0: 256 bytes (64B CFIS + 16B ACMD + 48B RSV + 128B PRDT)
    pub cmd_table_dma: DmaCoherent,
    pub lock: Mutex<()>,
}

impl AhciPort {
    /// Helper function to write arbitrary bytes into a VmWriter using a VmReader.
    fn write_bytes_to_dma(&self, writer: &mut VmWriter<'_, ostd::mm::Infallible>, data: &[u8]) {
        let mut reader = VmReader::from(data);
        let _ = writer.write(&mut reader);
    }

    /// Detects device type connected to port.
    pub fn probe_device_type(mmio: &IoMem, port_base: usize) -> Result<AhciDeviceType> {
        let ssts: u32 = mmio
            .read_once(port_base + PORT_SSTS)
            .map_err(|_| Errno::EIO)?;
        let ipm = (ssts >> 8) & 0x0F;
        let det = ssts & 0x0F;

        if det != HBA_PORT_DET_PRESENT || ipm != HBA_PORT_IPM_ACTIVE {
            if det != 0 || ipm != 0 {
                ostd::info!(
                    "AHCI: port_base {:#x} SSTS={:#x} (det={}, ipm={})",
                    port_base,
                    ssts,
                    det,
                    ipm
                );
            }
            return Ok(AhciDeviceType::None);
        }

        // Spin up and power on device if supported
        let mut cmd: u32 = mmio.read_once(port_base + PORT_CMD).unwrap_or(0);
        cmd |= PORT_CMD_SUD | PORT_CMD_POD;
        let _ = mmio.write_once(port_base + PORT_CMD, &cmd);

        let mut sig: u32 = mmio
            .read_once(port_base + PORT_SIG)
            .map_err(|_| Errno::EIO)?;
        ostd::info!(
            "AHCI: port_base {:#x} initial SSTS={:#x}, SIG={:#010x}",
            port_base,
            ssts,
            sig
        );

        // If signature is uninitialized, perform COMRESET
        if sig == 0xFFFF_FFFF || sig == 0 {
            // 1. Assert COMRESET (DET = 1)
            let sctl: u32 = mmio.read_once(port_base + PORT_SCTL).unwrap_or(0);
            let _ = mmio.write_once(port_base + PORT_SCTL, &((sctl & !0x0F) | 1));

            // Wait ~1ms
            for _ in 0..100_000 {
                core::hint::spin_loop();
            }

            // 2. Deassert COMRESET (DET = 0)
            let _ = mmio.write_once(port_base + PORT_SCTL, &(sctl & !0x0F));

            // 3. Wait for PHY communication to be re-established
            for _ in 0..200_000 {
                let cur_ssts: u32 = mmio.read_once(port_base + PORT_SSTS).unwrap_or(0);
                if (cur_ssts & 0x0F) == HBA_PORT_DET_PRESENT {
                    break;
                }
                core::hint::spin_loop();
            }

            // 4. Clear SATA errors (SERR is RWC)
            let _ = mmio.write_once(port_base + PORT_SERR, &0xFFFF_FFFFu32);

            // 5. Poll for signature update
            for _ in 0..200_000 {
                sig = mmio.read_once(port_base + PORT_SIG).unwrap_or(0);
                if sig != 0xFFFF_FFFF && sig != 0 {
                    break;
                }
                core::hint::spin_loop();
            }
            ostd::info!(
                "AHCI: port_base {:#x} after COMRESET SIG={:#010x}",
                port_base,
                sig
            );
        }

        let dev_type = match sig {
            SATA_SIG_ATA => AhciDeviceType::Sata,
            SATA_SIG_ATAPI => AhciDeviceType::Satapi,
            SATA_SIG_SEMB => AhciDeviceType::Semb,
            SATA_SIG_PM => AhciDeviceType::PortMultiplier,
            _ => {
                // If link is up (det == 3) but signature remains 0 or 0xFFFFFFFF,
                // check if it is a default SATA drive
                if det == HBA_PORT_DET_PRESENT && (sig == 0xFFFF_FFFF || sig == 0) {
                    let port_no = (port_base - AHCI_PORTS_BASE) / 0x80;
                    ostd::info!(
                        "AHCI: Port {} link active (SSTS={:#x}), identified as SATA drive",
                        port_no,
                        ssts
                    );
                    AhciDeviceType::Sata
                } else {
                    AhciDeviceType::Unknown
                }
            }
        };

        Ok(dev_type)
    }

    /// Creates and initializes an AHCI port with allocated DMA structures.
    pub fn new(port_no: usize, mmio: IoMem, name: String) -> Result<Self> {
        let port_base = AHCI_PORTS_BASE + port_no * 0x80;

        // Allocate 1 page (4096 bytes) for Command List
        let cmd_list_dma = DmaCoherent::alloc(1, true).map_err(|_| Errno::ENOMEM)?;
        // Allocate 1 page for Received FIS buffer
        let rx_fis_dma = DmaCoherent::alloc(1, true).map_err(|_| Errno::ENOMEM)?;
        // Allocate 1 page for Command Table (Slot 0)
        let cmd_table_dma = DmaCoherent::alloc(1, true).map_err(|_| Errno::ENOMEM)?;

        let mut port = Self {
            port_no,
            port_base,
            name,
            sector_count: 0,
            sector_size: 512,
            mmio,
            cmd_list_dma,
            rx_fis_dma,
            cmd_table_dma,
            lock: Mutex::new(()),
        };

        port.rebase()?;
        port.identify()?;

        Ok(port)
    }

    /// Stops port command engine (ST and FRE cleared).
    fn stop_cmd(&self) -> Result<()> {
        let mut cmd: u32 = self
            .mmio
            .read_once(self.port_base + PORT_CMD)
            .map_err(|_| Errno::EIO)?;
        cmd &= !PORT_CMD_ST;
        cmd &= !PORT_CMD_FRE;
        self.mmio
            .write_once(self.port_base + PORT_CMD, &cmd)
            .map_err(|_| Errno::EIO)?;

        // Wait for Engine CR and FR bits to clear
        for _ in 0..100_000 {
            let current_cmd: u32 = self
                .mmio
                .read_once(self.port_base + PORT_CMD)
                .map_err(|_| Errno::EIO)?;
            if (current_cmd & (PORT_CMD_CR | PORT_CMD_FR)) == 0 {
                return Ok(());
            }
        }

        ostd::warn!(
            "AHCI: Port {} stop_cmd timed out waiting for CR/FR to clear",
            self.port_no
        );
        Ok(())
    }

    /// Starts port command engine (FRE and ST set).
    fn start_cmd(&self) -> Result<()> {
        // Wait until CR is cleared
        for _ in 0..100_000 {
            let cmd: u32 = self
                .mmio
                .read_once(self.port_base + PORT_CMD)
                .map_err(|_| Errno::EIO)?;
            if (cmd & PORT_CMD_CR) == 0 {
                break;
            }
        }

        let mut cmd: u32 = self
            .mmio
            .read_once(self.port_base + PORT_CMD)
            .map_err(|_| Errno::EIO)?;
        cmd |= PORT_CMD_FRE;
        self.mmio
            .write_once(self.port_base + PORT_CMD, &cmd)
            .map_err(|_| Errno::EIO)?;

        cmd |= PORT_CMD_ST;
        self.mmio
            .write_once(self.port_base + PORT_CMD, &cmd)
            .map_err(|_| Errno::EIO)?;
        Ok(())
    }

    /// Configures CLB, FB, and Command Table addresses according to OSDev AHCI specification.
    pub fn rebase(&mut self) -> Result<()> {
        self.stop_cmd()?;

        let clb_phys = self.cmd_list_dma.paddr() as u64;
        let fb_phys = self.rx_fis_dma.paddr() as u64;
        let ctba_phys = self.cmd_table_dma.paddr() as u64;

        // Set Command List Base
        let clb_low = clb_phys as u32;
        let clb_high = (clb_phys >> 32) as u32;
        self.mmio
            .write_once(self.port_base + PORT_CLB, &clb_low)
            .map_err(|_| Errno::EIO)?;
        self.mmio
            .write_once(self.port_base + PORT_CLBU, &clb_high)
            .map_err(|_| Errno::EIO)?;

        // Set FIS Base
        let fb_low = fb_phys as u32;
        let fb_high = (fb_phys >> 32) as u32;
        self.mmio
            .write_once(self.port_base + PORT_FB, &fb_low)
            .map_err(|_| Errno::EIO)?;
        self.mmio
            .write_once(self.port_base + PORT_FBU, &fb_high)
            .map_err(|_| Errno::EIO)?;

        // Set up Command Header 0 inside Command List
        // 5 Dwords FIS length (20 bytes), Write=0, PRDT length=1
        let cmd_header = HbaCmdHeader {
            flags: 5, // 5 dwords CFL
            prdtl: 1, // 1 PRD entry
            prdbc: 0,
            ctba: ctba_phys as u32,
            ctbau: (ctba_phys >> 32) as u32,
            rsv: [0; 4],
        };

        let mut writer = self.cmd_list_dma.writer();
        self.write_bytes_to_dma(&mut writer, &cmd_header.to_bytes());

        // Clear any pending interrupt status & error status
        let is: u32 = self.mmio.read_once(self.port_base + PORT_IS).unwrap_or(0);
        let _ = self.mmio.write_once(self.port_base + PORT_IS, &is);
        let serr: u32 = self.mmio.read_once(self.port_base + PORT_SERR).unwrap_or(0);
        let _ = self.mmio.write_once(self.port_base + PORT_SERR, &serr);

        self.start_cmd()?;

        ostd::info!("AHCI: Port {} rebased successfully", self.port_no);
        Ok(())
    }

    /// Wait for device to become ready (BUSY and DRQ cleared).
    fn wait_ready(&self) -> Result<()> {
        for _ in 0..100_000 {
            let tfd: u32 = self
                .mmio
                .read_once(self.port_base + PORT_TFD)
                .map_err(|_| Errno::EIO)?;
            if (tfd & (ATA_DEV_BUSY | ATA_DEV_DRQ)) == 0 {
                return Ok(());
            }
        }
        crate::return_errno!(EBUSY, "port is busy")
    }

    /// Sends ATA IDENTIFY DEVICE command to read capacity and parameters.
    pub fn identify(&mut self) -> Result<()> {
        let _guard = self.lock.lock();
        self.wait_ready()?;

        // Allocate a 1-page DMA buffer for IDENTIFY data (512 bytes)
        let dma_buf = DmaCoherent::alloc(1, true).map_err(|_| Errno::ENOMEM)?;

        // 1. Construct Command Table: CFIS (FisRegH2D) + PRDT
        let mut cfis = FisRegH2D::default();
        cfis.fis_type = FisType::RegH2D as u8;
        cfis.pmport_c = 0x80; // Command bit set
        cfis.command = ATA_CMD_IDENTIFY_DEVICE;
        cfis.device = 0;

        let prdt = HbaPrdtEntry {
            dba: dma_buf.paddr() as u32,
            dbau: (dma_buf.paddr() >> 32) as u32,
            rsv0: 0,
            dbc_i: (512 - 1) | (1 << 31), // 512 bytes, IOC set
        };

        // Write CFIS at offset 0 and PRDT at offset 128
        let mut table_writer = self.cmd_table_dma.writer();
        self.write_bytes_to_dma(&mut table_writer, &cfis.to_bytes());

        // Zero padding up to PRDT offset 128
        let zero_pad = [0u8; 128 - FisRegH2D::SIZE];
        self.write_bytes_to_dma(&mut table_writer, &zero_pad);

        self.write_bytes_to_dma(&mut table_writer, &prdt.to_bytes());

        // 2. Set Command Header in Command List
        let ctba_phys = self.cmd_table_dma.paddr() as u64;
        let cmd_header = HbaCmdHeader {
            flags: 5, // 5 Dwords, Write = 0
            prdtl: 1,
            prdbc: 0,
            ctba: ctba_phys as u32,
            ctbau: (ctba_phys >> 32) as u32,
            rsv: [0; 4],
        };

        let mut cl_writer = self.cmd_list_dma.writer();
        self.write_bytes_to_dma(&mut cl_writer, &cmd_header.to_bytes());

        // 3. Issue Command (Slot 0)
        let slot_mask: u32 = 1;
        self.mmio
            .write_once(self.port_base + PORT_CI, &slot_mask)
            .map_err(|_| Errno::EIO)?;

        // 4. Poll for completion
        let mut completed = false;
        for _ in 0..500_000 {
            let ci: u32 = self
                .mmio
                .read_once(self.port_base + PORT_CI)
                .map_err(|_| Errno::EIO)?;
            if (ci & 1) == 0 {
                completed = true;
                break;
            }
        }

        if !completed {
            crate::return_errno!(ETIMEDOUT, "ATA IDENTIFY command timed out");
        }

        // Read Identify data
        let mut id_raw = [0u8; 512];
        let mut reader = dma_buf.reader();
        let mut writer = VmWriter::from(&mut id_raw[..]);
        let _ = reader.read(&mut writer);

        // Word 100-103: Total number of user addressable sectors (LBA48)
        let w100 = u16::from_le_bytes([id_raw[200], id_raw[201]]) as u64;
        let w101 = u16::from_le_bytes([id_raw[202], id_raw[203]]) as u64;
        let w102 = u16::from_le_bytes([id_raw[204], id_raw[205]]) as u64;
        let w103 = u16::from_le_bytes([id_raw[206], id_raw[207]]) as u64;
        let lba48_sectors = w100 | (w101 << 16) | (w102 << 32) | (w103 << 48);

        // Word 60-61: Total number of user addressable sectors (LBA28)
        let w60 = u16::from_le_bytes([id_raw[120], id_raw[121]]) as u64;
        let w61 = u16::from_le_bytes([id_raw[122], id_raw[123]]) as u64;
        let lba28_sectors = w60 | (w61 << 16);

        self.sector_count = if lba48_sectors > 0 {
            lba48_sectors
        } else {
            lba28_sectors
        };
        self.sector_size = 512;

        ostd::info!(
            "AHCI: Port {} ATA IDENTIFY successful, sectors: {}, capacity: {} MB",
            self.port_no,
            self.sector_count,
            (self.sector_count * 512) / (1024 * 1024)
        );

        Ok(())
    }

    /// Read sectors using ATA READ DMA EXT (Command 0x25).
    pub fn read_dma_ext(&self, start_lba: u64, sector_count: u16, buf: &mut [u8]) -> Result<usize> {
        if sector_count == 0 {
            return Ok(0);
        }

        let _guard = self.lock.lock();
        self.wait_ready()?;

        let byte_count = sector_count as u32 * 512;
        let dma_frames = (byte_count as usize + ostd::mm::PAGE_SIZE - 1) / ostd::mm::PAGE_SIZE;
        let dma_buf = DmaCoherent::alloc(dma_frames, true).map_err(|_| Errno::ENOMEM)?;

        // 1. Construct CFIS
        let mut cfis = FisRegH2D::default();
        cfis.fis_type = FisType::RegH2D as u8;
        cfis.pmport_c = 0x80;
        cfis.command = ATA_CMD_READ_DMA_EXT;
        cfis.device = ATA_DEV_LBA;

        cfis.lba0 = (start_lba & 0xFF) as u8;
        cfis.lba1 = ((start_lba >> 8) & 0xFF) as u8;
        cfis.lba2 = ((start_lba >> 16) & 0xFF) as u8;
        cfis.lba3 = ((start_lba >> 24) & 0xFF) as u8;
        cfis.lba4 = ((start_lba >> 32) & 0xFF) as u8;
        cfis.lba5 = ((start_lba >> 40) & 0xFF) as u8;

        cfis.count_l = (sector_count & 0xFF) as u8;
        cfis.count_h = ((sector_count >> 8) & 0xFF) as u8;

        // 2. PRDT entry
        let prdt = HbaPrdtEntry {
            dba: dma_buf.paddr() as u32,
            dbau: (dma_buf.paddr() >> 32) as u32,
            rsv0: 0,
            dbc_i: (byte_count - 1) | (1 << 31),
        };

        // Write CFIS + PRDT to command table
        let mut table_writer = self.cmd_table_dma.writer();
        self.write_bytes_to_dma(&mut table_writer, &cfis.to_bytes());

        let zero_pad = [0u8; 128 - FisRegH2D::SIZE];
        self.write_bytes_to_dma(&mut table_writer, &zero_pad);

        self.write_bytes_to_dma(&mut table_writer, &prdt.to_bytes());

        // 3. Command Header (Write = 0)
        let ctba_phys = self.cmd_table_dma.paddr() as u64;
        let cmd_header = HbaCmdHeader {
            flags: 5, // Read: Write bit 6 is 0
            prdtl: 1,
            prdbc: 0,
            ctba: ctba_phys as u32,
            ctbau: (ctba_phys >> 32) as u32,
            rsv: [0; 4],
        };

        let mut cl_writer = self.cmd_list_dma.writer();
        self.write_bytes_to_dma(&mut cl_writer, &cmd_header.to_bytes());

        // 4. Issue command
        self.mmio
            .write_once(self.port_base + PORT_CI, &1u32)
            .map_err(|_| Errno::EIO)?;

        // 5. Poll for completion
        let mut completed = false;
        for _ in 0..1_000_000 {
            let ci: u32 = self
                .mmio
                .read_once(self.port_base + PORT_CI)
                .map_err(|_| Errno::EIO)?;
            if (ci & 1) == 0 {
                completed = true;
                break;
            }
        }

        if !completed {
            crate::return_errno!(ETIMEDOUT, "AHCI read_dma_ext command timed out");
        }

        // Copy read data from DMA buffer to caller's buffer
        let mut reader = dma_buf.reader();
        let mut writer = VmWriter::from(buf);
        let bytes_copied = reader.read(&mut writer);

        Ok(bytes_copied)
    }

    /// Write sectors using ATA WRITE DMA EXT (Command 0x35).
    pub fn write_dma_ext(&self, start_lba: u64, sector_count: u16, buf: &[u8]) -> Result<usize> {
        if sector_count == 0 {
            return Ok(0);
        }

        let _guard = self.lock.lock();
        self.wait_ready()?;

        let byte_count = sector_count as u32 * 512;
        let dma_frames = (byte_count as usize + ostd::mm::PAGE_SIZE - 1) / ostd::mm::PAGE_SIZE;
        let dma_buf = DmaCoherent::alloc(dma_frames, true).map_err(|_| Errno::ENOMEM)?;

        // Stage data into DMA buffer
        let mut reader = VmReader::from(buf);
        let mut writer = dma_buf.writer();
        let _ = reader.read(&mut writer);

        // 1. Construct CFIS
        let mut cfis = FisRegH2D::default();
        cfis.fis_type = FisType::RegH2D as u8;
        cfis.pmport_c = 0x80;
        cfis.command = ATA_CMD_WRITE_DMA_EXT;
        cfis.device = ATA_DEV_LBA;

        cfis.lba0 = (start_lba & 0xFF) as u8;
        cfis.lba1 = ((start_lba >> 8) & 0xFF) as u8;
        cfis.lba2 = ((start_lba >> 16) & 0xFF) as u8;
        cfis.lba3 = ((start_lba >> 24) & 0xFF) as u8;
        cfis.lba4 = ((start_lba >> 32) & 0xFF) as u8;
        cfis.lba5 = ((start_lba >> 40) & 0xFF) as u8;

        cfis.count_l = (sector_count & 0xFF) as u8;
        cfis.count_h = ((sector_count >> 8) & 0xFF) as u8;

        // 2. PRDT entry
        let prdt = HbaPrdtEntry {
            dba: dma_buf.paddr() as u32,
            dbau: (dma_buf.paddr() >> 32) as u32,
            rsv0: 0,
            dbc_i: (byte_count - 1) | (1 << 31),
        };

        // Write CFIS + PRDT to command table
        let mut table_writer = self.cmd_table_dma.writer();
        self.write_bytes_to_dma(&mut table_writer, &cfis.to_bytes());

        let zero_pad = [0u8; 128 - FisRegH2D::SIZE];
        self.write_bytes_to_dma(&mut table_writer, &zero_pad);

        self.write_bytes_to_dma(&mut table_writer, &prdt.to_bytes());

        // 3. Command Header (Write = 1 -> Bit 6 set: 5 | (1 << 6) = 0x45)
        let ctba_phys = self.cmd_table_dma.paddr() as u64;
        let cmd_header = HbaCmdHeader {
            flags: 5 | (1 << 6), // Write bit set
            prdtl: 1,
            prdbc: 0,
            ctba: ctba_phys as u32,
            ctbau: (ctba_phys >> 32) as u32,
            rsv: [0; 4],
        };

        let mut cl_writer = self.cmd_list_dma.writer();
        self.write_bytes_to_dma(&mut cl_writer, &cmd_header.to_bytes());

        // 4. Issue command
        self.mmio
            .write_once(self.port_base + PORT_CI, &1u32)
            .map_err(|_| Errno::EIO)?;

        // 5. Poll for completion
        let mut completed = false;
        for _ in 0..1_000_000 {
            let ci: u32 = self
                .mmio
                .read_once(self.port_base + PORT_CI)
                .map_err(|_| Errno::EIO)?;
            if (ci & 1) == 0 {
                completed = true;
                break;
            }
        }

        if !completed {
            crate::return_errno!(ETIMEDOUT, "AHCI write_dma_ext command timed out");
        }

        Ok(byte_count as usize)
    }
}

impl Device for AhciPort {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Block
    }
}

impl BlockDevice for AhciPort {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn block_size(&self) -> usize {
        self.sector_size
    }

    fn block_count(&self) -> u64 {
        self.sector_count
    }

    fn read_blocks(&self, block_id: u64, buf: &mut [u8]) -> Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if buf.len() % self.sector_size != 0 {
            crate::return_errno!(EINVAL, "buffer length must be a multiple of sector size");
        }

        let sector_count = (buf.len() / self.sector_size) as u16;
        self.read_dma_ext(block_id, sector_count, buf)
    }

    fn write_blocks(&self, block_id: u64, buf: &[u8]) -> Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if buf.len() % self.sector_size != 0 {
            crate::return_errno!(EINVAL, "buffer length must be a multiple of sector size");
        }

        let sector_count = (buf.len() / self.sector_size) as u16;
        self.write_dma_ext(block_id, sector_count, buf)
    }

    fn flush(&self) -> Result<()> {
        Ok(())
    }
}
