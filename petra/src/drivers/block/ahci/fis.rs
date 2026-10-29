// SPDX-License-Identifier: GPL-2.0

//! Frame Information Structure (FIS) Definitions.
//!
//! Based on Serial ATA Advanced Host Controller Interface (AHCI) Specification
//! and Serial ATA Revision 3.x Specification.

/// FIS Types defined by Serial ATA Specification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FisType {
    /// Register FIS - Host to Device
    RegH2D = 0x27,
    /// Register FIS - Device to Host
    RegD2H = 0x34,
    /// DMA Activate FIS - Device to Host
    DmaAct = 0x39,
    /// DMA Setup FIS - Bidirectional
    DmaSetup = 0x41,
    /// Data FIS - Bidirectional
    Data = 0x46,
    /// BIST Activate FIS - Bidirectional
    Bist = 0x58,
    /// PIO Setup FIS - Device to Host
    PioSetup = 0x5F,
    /// Set Device Bits FIS - Device to Host
    DevBits = 0xA1,
}

/// Register FIS - Host to Device (20 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct FisRegH2D {
    /// FIS Type (FisType::RegH2D = 0x27)
    pub fis_type: u8,
    /// Port multiplier (bits 0:3), reserved (bits 4:6), Command bit (bit 7: 1 = Command, 0 = Control)
    pub pmport_c: u8,
    /// ATA Command register
    pub command: u8,
    /// Feature register (7:0)
    pub feature_l: u8,

    /// LBA low register (7:0)
    pub lba0: u8,
    /// LBA mid register (15:8)
    pub lba1: u8,
    /// LBA high register (23:16)
    pub lba2: u8,
    /// Device register (bit 6 = 1 for LBA mode)
    pub device: u8,

    /// LBA register (31:24)
    pub lba3: u8,
    /// LBA register (39:32)
    pub lba4: u8,
    /// LBA register (47:40)
    pub lba5: u8,
    /// Feature register (15:8)
    pub feature_h: u8,

    /// Sector count (7:0)
    pub count_l: u8,
    /// Sector count (15:8)
    pub count_h: u8,
    /// Isochronous command completion / reserved
    pub icc: u8,
    /// Control register
    pub control: u8,

    /// Reserved (must be 0)
    pub rsv1: [u8; 4],
}

impl FisRegH2D {
    pub const SIZE: usize = 20;

    /// Serialize to 20-byte array without unsafe code.
    pub fn to_bytes(&self) -> [u8; 20] {
        [
            self.fis_type,
            self.pmport_c,
            self.command,
            self.feature_l,
            self.lba0,
            self.lba1,
            self.lba2,
            self.device,
            self.lba3,
            self.lba4,
            self.lba5,
            self.feature_h,
            self.count_l,
            self.count_h,
            self.icc,
            self.control,
            self.rsv1[0],
            self.rsv1[1],
            self.rsv1[2],
            self.rsv1[3],
        ]
    }
}
