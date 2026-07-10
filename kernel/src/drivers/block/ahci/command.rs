/// AHCI Command Construction and Execution
///
/// Builds and sends ATA commands to SATA devices through the AHCI HBA.
/// Supports both read (READ DMA EXT) and write (WRITE DMA EXT) operations,
/// as well as IDENTIFY DEVICE for querying disk geometry.
use ostd::io::IoMem;
use ostd::mm::dma::DmaCoherent;
use ostd::mm::{HasDaddr, VmIo, VmIoOnce};

use super::hba;

// ──────────────────────────────────────────────────────────────
// ATA command codes
// ──────────────────────────────────────────────────────────────

/// READ DMA EXT — 48-bit LBA DMA read.
pub const ATA_CMD_READ_DMA_EXT: u8 = 0x25;
/// WRITE DMA EXT — 48-bit LBA DMA write.
pub const ATA_CMD_WRITE_DMA_EXT: u8 = 0x35;
/// IDENTIFY DEVICE — returns 512 bytes of device information.
pub const ATA_CMD_IDENTIFY: u8 = 0xEC;

// FIS (Frame Information Structure) constants
/// Register Host-to-Device FIS type.
const FIS_TYPE_REG_H2D: u8 = 0x27;
/// Command bit — set in byte 1 to indicate this is a command (not control).
const FIS_CMD_BIT: u8 = 0x80;
/// LBA mode bit — set in the device register byte.
const LBA_MODE: u8 = 0x40;

// Command header flags
/// Write direction flag in command header DW0.
const CMD_HDR_WRITE: u32 = 1 << 6;
/// PRDT length of 1 in command header DW0 (one scatter/gather entry).
const CMD_HDR_PRDTL_1: u32 = 1 << 16;
/// Command FIS length = 5 DWORDs (20 bytes) for H2D Register FIS.
const CMD_HDR_CFL: u32 = 5;

/// PRDT interrupt-on-completion flag.
const PRDT_IOC: u32 = 1 << 31;

/// Result of an IDENTIFY DEVICE command.
pub struct DeviceIdentity {
    /// Total number of addressable sectors on the device.
    pub sector_count: u64,
    /// Logical sector size in bytes (typically 512).
    pub sector_size: usize,
}

/// Send an ATA command to a port via the AHCI command engine.
///
/// This builds a command header + command table + PRDT entry, then
/// issues the command and polls for completion.
///
/// # Arguments
/// - `write` — `true` for a write command, `false` for read
/// - `lba` — Starting logical block address (48-bit)
/// - `count` — Number of sectors to transfer
pub fn send_command(
    abar: &IoMem,
    port: usize,
    cmd_list: &DmaCoherent,
    cmd_table: &DmaCoherent,
    dma_buf: &DmaCoherent,
    write: bool,
    lba: u64,
    count: u16,
) -> Result<(), ostd::Error> {
    let base = hba::port_offset(port);

    // Clear any pending interrupt status
    abar.write_once(base + hba::PORT_IS, &0xFFFF_FFFFu32)?;

    // ── Build command header (DW0-DW7) in the command list ──
    let write_flag = if write { CMD_HDR_WRITE } else { 0 };
    let dw0 = CMD_HDR_CFL | write_flag | CMD_HDR_PRDTL_1;
    cmd_list.write_once(0, &dw0)?;
    // DW1: Physical Region Descriptor Byte Count (cleared, filled by HBA)
    cmd_list.write_once(4, &0u32)?;

    // DW2-DW3: Command Table Base Address (64-bit)
    let ct_daddr = cmd_table.daddr() as u64;
    cmd_list.write_once(8, &(ct_daddr as u32))?;
    cmd_list.write_once(12, &((ct_daddr >> 32) as u32))?;

    // DW4-DW7: Reserved, zero them out
    for i in 4..8 {
        cmd_list.write_once(i * 4, &0u32)?;
    }

    // ── Build Command FIS (Register H2D) in the command table ──
    let ata_cmd = if write {
        ATA_CMD_WRITE_DMA_EXT
    } else {
        ATA_CMD_READ_DMA_EXT
    };
    let mut cfis = [0u8; 20];
    cfis[0] = FIS_TYPE_REG_H2D;
    cfis[1] = FIS_CMD_BIT;
    cfis[2] = ata_cmd;

    // LBA bytes [0:23]
    cfis[4] = (lba & 0xFF) as u8;
    cfis[5] = ((lba >> 8) & 0xFF) as u8;
    cfis[6] = ((lba >> 16) & 0xFF) as u8;
    cfis[7] = LBA_MODE;

    // LBA bytes [24:47]
    cfis[8] = ((lba >> 24) & 0xFF) as u8;
    cfis[9] = ((lba >> 32) & 0xFF) as u8;
    cfis[10] = ((lba >> 40) & 0xFF) as u8;

    // Sector count
    cfis[12] = (count & 0xFF) as u8;
    cfis[13] = ((count >> 8) & 0xFF) as u8;

    cmd_table.write_bytes(0, &cfis)?;

    // ── Build PRDT entry at offset 0x80 in the command table ──
    let buf_daddr = dma_buf.daddr() as u64;
    cmd_table.write_once(0x80, &(buf_daddr as u32))?; // Data Base Address
    cmd_table.write_once(0x84, &((buf_daddr >> 32) as u32))?; // Data Base Address Upper
    cmd_table.write_once(0x88, &0u32)?; // Reserved
    let byte_count = (count as u32 * 512) - 1;
    cmd_table.write_once(0x8C, &(byte_count | PRDT_IOC))?; // DBC + IOC

    // ── Issue the command (slot 0) ──
    abar.write_once(base + hba::PORT_CI, &1u32)?;

    // ── Poll for completion ──
    loop {
        let ci: u32 = abar.read_once(base + hba::PORT_CI)?;
        if (ci & 1) == 0 {
            break;
        }
        // Check Task File Data for errors
        let tfd: u32 = abar.read_once(base + hba::PORT_TFD)?;
        if (tfd & 0x01) != 0 {
            return Err(ostd::Error::IoError);
        }
    }

    Ok(())
}

/// Send an IDENTIFY DEVICE command and parse the result.
///
/// Returns the device identity including total sector count and sector size.
/// The IDENTIFY DEVICE command returns 512 bytes of device information that
/// contains the disk geometry.
pub fn identify_device(
    abar: &IoMem,
    port: usize,
    cmd_list: &DmaCoherent,
    cmd_table: &DmaCoherent,
    dma_buf: &DmaCoherent,
) -> Result<DeviceIdentity, ostd::Error> {
    let base = hba::port_offset(port);

    // Clear interrupt status
    abar.write_once(base + hba::PORT_IS, &0xFFFF_FFFFu32)?;

    // Build command header for IDENTIFY (read, no PRDT write flag)
    let dw0 = CMD_HDR_CFL | CMD_HDR_PRDTL_1;
    cmd_list.write_once(0, &dw0)?;
    cmd_list.write_once(4, &0u32)?;

    let ct_daddr = cmd_table.daddr() as u64;
    cmd_list.write_once(8, &(ct_daddr as u32))?;
    cmd_list.write_once(12, &((ct_daddr >> 32) as u32))?;

    for i in 4..8 {
        cmd_list.write_once(i * 4, &0u32)?;
    }

    // Build IDENTIFY DEVICE FIS
    let mut cfis = [0u8; 20];
    cfis[0] = FIS_TYPE_REG_H2D;
    cfis[1] = FIS_CMD_BIT;
    cfis[2] = ATA_CMD_IDENTIFY;
    cmd_table.write_bytes(0, &cfis)?;

    // PRDT: 512 bytes of identify data
    let buf_daddr = dma_buf.daddr() as u64;
    cmd_table.write_once(0x80, &(buf_daddr as u32))?;
    cmd_table.write_once(0x84, &((buf_daddr >> 32) as u32))?;
    cmd_table.write_once(0x88, &0u32)?;
    cmd_table.write_once(0x8C, &(511u32 | PRDT_IOC))?;

    // Issue command
    abar.write_once(base + hba::PORT_CI, &1u32)?;

    // Poll for completion
    loop {
        let ci: u32 = abar.read_once(base + hba::PORT_CI)?;
        if (ci & 1) == 0 {
            break;
        }
        let tfd: u32 = abar.read_once(base + hba::PORT_TFD)?;
        if (tfd & 0x01) != 0 {
            return Err(ostd::Error::IoError);
        }
    }

    // Read the 512 bytes of identify data
    let mut identify = [0u8; 512];
    dma_buf.read_bytes(0, &mut identify)?;

    // Parse sector count:
    // Words 100-103 (bytes 200-207): 48-bit LBA total sectors (u64)
    let lba48 = u64::from_le_bytes([
        identify[200],
        identify[201],
        identify[202],
        identify[203],
        identify[204],
        identify[205],
        identify[206],
        identify[207],
    ]);

    // Words 60-61 (bytes 120-123): 28-bit LBA total sectors (u32)
    let lba28 =
        u32::from_le_bytes([identify[120], identify[121], identify[122], identify[123]]) as u64;

    let sector_count = if lba48 > 0 { lba48 } else { lba28 };

    // Parse sector size:
    // Word 106 (bytes 212-213): Physical/logical sector size
    let word106 = u16::from_le_bytes([identify[212], identify[213]]);
    let sector_size = if (word106 & (1 << 12)) != 0 {
        // Logical sector size is in words 117-118 (bytes 234-237)
        let words_per_sector =
            u32::from_le_bytes([identify[234], identify[235], identify[236], identify[237]]);
        (words_per_sector as usize) * 2
    } else {
        512
    };

    // Ensure we have reasonable defaults
    let sector_count = if sector_count == 0 {
        1024
    } else {
        sector_count
    };
    let sector_size = if sector_size == 0 { 512 } else { sector_size };

    Ok(DeviceIdentity {
        sector_count,
        sector_size,
    })
}
