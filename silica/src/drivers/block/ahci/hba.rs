// SPDX-License-Identifier: GPL-2.0

//! AHCI Register Constants and Low-Level Command Definitions.
//!
//! Based on the AHCI 1.3.1 Specification and OSDev AHCI reference.

// ----------------------------------------------------------------------------
// Generic Host Control (GHC) Register Offsets
// ----------------------------------------------------------------------------
pub const AHCI_CAP: usize = 0x00; // Host Capability
pub const AHCI_GHC: usize = 0x04; // Global Host Control
pub const AHCI_IS: usize = 0x08; // Interrupt Status
pub const AHCI_PI: usize = 0x0C; // Ports Implemented
pub const AHCI_VS: usize = 0x10; // AHCI Version
pub const AHCI_PORTS_BASE: usize = 0x100; // Base offset of Port 0

// GHC Flags
pub const GHC_HR: u32 = 1 << 0; // HBA Reset
pub const GHC_IE: u32 = 1 << 1; // Interrupt Enable
pub const GHC_AE: u32 = 1 << 31; // AHCI Enable

// ----------------------------------------------------------------------------
// Port Register Offsets (relative to port_base = 0x100 + port_no * 0x80)
// ----------------------------------------------------------------------------
pub const PORT_CLB: usize = 0x00; // Command List Base Address (1KB aligned)
pub const PORT_CLBU: usize = 0x04; // Command List Base Address Upper 32-bits
pub const PORT_FB: usize = 0x08; // FIS Base Address (256B aligned)
pub const PORT_FBU: usize = 0x0C; // FIS Base Address Upper 32-bits
pub const PORT_IS: usize = 0x10; // Interrupt Status
pub const PORT_IE: usize = 0x14; // Interrupt Enable
pub const PORT_CMD: usize = 0x18; // Command and Status
pub const PORT_TFD: usize = 0x20; // Task File Data
pub const PORT_SIG: usize = 0x24; // Signature
pub const PORT_SSTS: usize = 0x28; // Serial ATA Status (SCR0: SStatus)
pub const PORT_SCTL: usize = 0x2C; // Serial ATA Control (SCR2: SControl)
pub const PORT_SERR: usize = 0x30; // Serial ATA Error (SCR1: SError)
pub const PORT_SACT: usize = 0x34; // Serial ATA Active (SCR3: SActive)
pub const PORT_CI: usize = 0x38; // Command Issue

// Port CMD Register Flags
pub const PORT_CMD_ST: u32 = 1 << 0; // Start Command Engine
pub const PORT_CMD_SUD: u32 = 1 << 1; // Spin-Up Device
pub const PORT_CMD_POD: u32 = 1 << 2; // Power On Device
pub const PORT_CMD_FRE: u32 = 1 << 4; // FIS Receive Enable
pub const PORT_CMD_FR: u32 = 1 << 14; // FIS Receive Running
pub const PORT_CMD_CR: u32 = 1 << 15; // Command List Running

// Port TFD Flags
pub const ATA_DEV_BUSY: u32 = 0x80;
pub const ATA_DEV_DRQ: u32 = 0x08;

// ATA Commands
pub const ATA_CMD_READ_DMA_EXT: u8 = 0x25;
pub const ATA_CMD_WRITE_DMA_EXT: u8 = 0x35;
pub const ATA_CMD_IDENTIFY_DEVICE: u8 = 0xEC;
pub const ATA_DEV_LBA: u8 = 0x40;

// Device Signatures
pub const SATA_SIG_ATA: u32 = 0x00000101;
pub const SATA_SIG_ATAPI: u32 = 0xEB140101;
pub const SATA_SIG_SEMB: u32 = 0xC33C0101;
pub const SATA_SIG_PM: u32 = 0x96690101;

// Port Status
pub const HBA_PORT_DET_PRESENT: u32 = 3;
pub const HBA_PORT_IPM_ACTIVE: u32 = 1;

// ----------------------------------------------------------------------------
// In-Memory Structures (Command Header, PRDT, Command Table)
// ----------------------------------------------------------------------------

/// Command Header in Command List (32 bytes per slot).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct HbaCmdHeader {
    /// Command FIS length in DWORDS (bits 0:4), ATAPI bit 5, Write bit 6, Prefetch bit 7, etc.
    pub flags: u16,
    /// Physical Region Descriptor Table Length (number of PRDT entries).
    pub prdtl: u16,
    /// Physical Region Descriptor Byte Count transferred.
    pub prdbc: u32,
    /// Command Table Base Address (128-byte aligned).
    pub ctba: u32,
    /// Command Table Base Address Upper 32-bits.
    pub ctbau: u32,
    /// Reserved (16 bytes).
    pub rsv: [u32; 4],
}

impl HbaCmdHeader {
    pub const SIZE: usize = 32;

    /// Serialize to 32-byte array without unsafe code.
    pub fn to_bytes(&self) -> [u8; 32] {
        let mut b = [0u8; 32];
        b[0..2].copy_from_slice(&self.flags.to_le_bytes());
        b[2..4].copy_from_slice(&self.prdtl.to_le_bytes());
        b[4..8].copy_from_slice(&self.prdbc.to_le_bytes());
        b[8..12].copy_from_slice(&self.ctba.to_le_bytes());
        b[12..16].copy_from_slice(&self.ctbau.to_le_bytes());
        b[16..20].copy_from_slice(&self.rsv[0].to_le_bytes());
        b[20..24].copy_from_slice(&self.rsv[1].to_le_bytes());
        b[24..28].copy_from_slice(&self.rsv[2].to_le_bytes());
        b[28..32].copy_from_slice(&self.rsv[3].to_le_bytes());
        b
    }
}

/// Physical Region Descriptor Table (PRDT) Entry (16 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct HbaPrdtEntry {
    /// Data Base Address.
    pub dba: u32,
    /// Data Base Address Upper 32-bits.
    pub dbau: u32,
    /// Reserved.
    pub rsv0: u32,
    /// Byte count - 1 (bits 0:21), bit 31 = Interrupt on completion (IOC).
    pub dbc_i: u32,
}

impl HbaPrdtEntry {
    pub const SIZE: usize = 16;

    /// Serialize to 16-byte array without unsafe code.
    pub fn to_bytes(&self) -> [u8; 16] {
        let mut b = [0u8; 16];
        b[0..4].copy_from_slice(&self.dba.to_le_bytes());
        b[4..8].copy_from_slice(&self.dbau.to_le_bytes());
        b[8..12].copy_from_slice(&self.rsv0.to_le_bytes());
        b[12..16].copy_from_slice(&self.dbc_i.to_le_bytes());
        b
    }
}
