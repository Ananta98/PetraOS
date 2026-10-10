// SPDX-License-Identifier: GPL-2.0

//! 100% Safe xHCI Register and Memory Access Abstractions.
//!
//! Provides type-safe, hardware-verified register accessors directly over
//! [`ostd::io::IoMem`] using `VmIoOnce`, completely eliminating the need for
//! raw pointers, memory transmutation, and `unsafe` code.

use ostd::{io::IoMem, mm::VmIoOnce};
use crate::api::errno::{Errno, Result};

// ----------------------------------------------------------------------------
// Operational USBCMD & USBSTS Flags
// ----------------------------------------------------------------------------

pub const USBCMD_RUN_STOP: u32 = 1 << 0;
pub const USBCMD_HC_RESET: u32 = 1 << 1;
pub const USBCMD_INTE: u32 = 1 << 2;

pub const USBSTS_HC_HALTED: u32 = 1 << 0;
pub const USBSTS_CNR: u32 = 1 << 11; // Controller Not Ready

// ----------------------------------------------------------------------------
// Port Status & Control (PORTSC) Flags
// ----------------------------------------------------------------------------

pub const PORTSC_CCS: u32 = 1 << 0;          // Current Connect Status
pub const PORTSC_PED: u32 = 1 << 1;          // Port Enabled/Disabled
pub const PORTSC_PR: u32 = 1 << 4;           // Port Reset
pub const PORTSC_PP: u32 = 1 << 9;           // Port Power
pub const PORTSC_SPEED_MASK: u32 = 0x0F << 10;
pub const PORTSC_SPEED_SHIFT: u32 = 10;

// RW1C status change bits (must write 0 when modifying other control bits)
pub const PORTSC_RW1C_MASK: u32 = (1 << 1) | (0x7F << 17);

// ----------------------------------------------------------------------------
// xHCI Safe Register Structure
// ----------------------------------------------------------------------------

/// Type-safe, safe-Rust xHCI register offsets and operations.
#[derive(Debug, Clone, Copy)]
pub struct XhciRegisters {
    pub op_base: usize,
    pub rt_base: usize,
    pub db_base: usize,
    pub num_ports: u8,
    pub max_device_slots: u8,
    pub is_csz_64: bool,
}

impl XhciRegisters {
    /// Reads capability registers from `IoMem` and discovers operational,
    /// runtime, and doorbell base offsets.
    pub fn new(mmio: &IoMem) -> Result<Self> {
        // 1. CAPLENGTH (Offset 0x00, 1 byte)
        let caplength: u8 = mmio.read_once(0x00).map_err(|_| Errno::EIO)?;
        let op_base = caplength as usize;

        // 2. HCSPARAMS1 (Offset 0x04, 4 bytes)
        let hcsparams1: u32 = mmio.read_once(0x04).map_err(|_| Errno::EIO)?;
        let max_device_slots = (hcsparams1 & 0xFF) as u8;
        let num_ports = ((hcsparams1 >> 24) & 0xFF) as u8;

        // 3. HCCPARAMS1 (Offset 0x10, 4 bytes)
        let hccparams1: u32 = mmio.read_once(0x10).map_err(|_| Errno::EIO)?;
        let is_csz_64 = (hccparams1 & 0x04) != 0; // Context Size (1 = 64 bytes, 0 = 32 bytes)

        // 4. DBOFF (Offset 0x14, 4 bytes) - Doorbell Array Offset
        let dboff: u32 = mmio.read_once(0x14).map_err(|_| Errno::EIO)?;
        let db_base = (dboff & !0x03) as usize;

        // 5. RTSOFF (Offset 0x18, 4 bytes) - Runtime Register Space Offset
        let rtsoff: u32 = mmio.read_once(0x18).map_err(|_| Errno::EIO)?;
        let rt_base = (rtsoff & !0x1F) as usize;

        ostd::info!(
            "xHCI MMIO: OP at {:#x}, RT at {:#x}, DB at {:#x} (Ports: {}, Slots: {}, 64-bit CSZ: {})",
            op_base, rt_base, db_base, num_ports, max_device_slots, is_csz_64
        );

        Ok(Self {
            op_base,
            rt_base,
            db_base,
            num_ports,
            max_device_slots,
            is_csz_64,
        })
    }

    // ------------------------------------------------------------------------
    // Operational Registers
    // ------------------------------------------------------------------------

    pub fn read_usbcmd(&self, mmio: &IoMem) -> Result<u32> {
        mmio.read_once(self.op_base).map_err(|_| Errno::EIO)
    }

    pub fn write_usbcmd(&self, mmio: &IoMem, val: u32) -> Result<()> {
        mmio.write_once(self.op_base, &val).map_err(|_| Errno::EIO)
    }

    pub fn read_usbsts(&self, mmio: &IoMem) -> Result<u32> {
        mmio.read_once(self.op_base + 0x04).map_err(|_| Errno::EIO)
    }

    pub fn write_config(&self, mmio: &IoMem, max_slots_en: u8) -> Result<()> {
        let val = max_slots_en as u32;
        mmio.write_once(self.op_base + 0x38, &val).map_err(|_| Errno::EIO)
    }

    pub fn write_dcbaap(&self, mmio: &IoMem, paddr: u64) -> Result<()> {
        mmio.write_once(self.op_base + 0x30, &paddr).map_err(|_| Errno::EIO)
    }

    pub fn write_crcr(&self, mmio: &IoMem, cmd_ring_paddr: u64, ring_cycle_state: bool) -> Result<()> {
        let val = (cmd_ring_paddr & !0x3F) | (if ring_cycle_state { 1 } else { 0 });
        mmio.write_once(self.op_base + 0x18, &val).map_err(|_| Errno::EIO)
    }

    // ------------------------------------------------------------------------
    // Interrupter 0 (Primary Event Ring)
    // ------------------------------------------------------------------------

    pub fn setup_interrupter_0(
        &self,
        mmio: &IoMem,
        erst_sz: u32,
        erstba_paddr: u64,
        erdp_paddr: u64,
    ) -> Result<()> {
        let intr0 = self.rt_base + 0x20;
        // ERSTSZ (Event Ring Segment Table Size)
        mmio.write_once(intr0 + 0x08, &erst_sz).map_err(|_| Errno::EIO)?;
        // ERSTBA (Event Ring Segment Table Base Address)
        mmio.write_once(intr0 + 0x10, &erstba_paddr).map_err(|_| Errno::EIO)?;
        // ERDP (Event Ring Dequeue Pointer) - write bit 3 (EHB) to clear busy flag
        let erdp = (erdp_paddr & !0x0F) | 0x08;
        mmio.write_once(intr0 + 0x18, &erdp).map_err(|_| Errno::EIO)?;
        // IMAN (Interrupt Management: bit 1 = Interrupt Enable)
        let iman: u32 = 0x02;
        mmio.write_once(intr0 + 0x00, &iman).map_err(|_| Errno::EIO)?;
        Ok(())
    }

    pub fn update_erdp(&self, mmio: &IoMem, erdp_paddr: u64) -> Result<()> {
        let intr0_erdp = self.rt_base + 0x20 + 0x18;
        let erdp = (erdp_paddr & !0x0F) | 0x08;
        mmio.write_once(intr0_erdp, &erdp).map_err(|_| Errno::EIO)
    }

    // ------------------------------------------------------------------------
    // Doorbell Registers
    // ------------------------------------------------------------------------

    pub fn ring_doorbell(&self, mmio: &IoMem, slot_id: u8, target: u8) -> Result<()> {
        let db_reg = self.db_base + (slot_id as usize) * 4;
        let val = target as u32;
        mmio.write_once(db_reg, &val).map_err(|_| Errno::EIO)
    }

    // ------------------------------------------------------------------------
    // Port Register Sets (0-indexed)
    // ------------------------------------------------------------------------

    pub fn read_portsc(&self, mmio: &IoMem, port_idx: usize) -> Result<u32> {
        let reg = self.op_base + 0x400 + port_idx * 0x10;
        mmio.read_once(reg).map_err(|_| Errno::EIO)
    }

    pub fn write_portsc(&self, mmio: &IoMem, port_idx: usize, val: u32) -> Result<()> {
        let reg = self.op_base + 0x400 + port_idx * 0x10;
        mmio.write_once(reg, &val).map_err(|_| Errno::EIO)
    }
}
