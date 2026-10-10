// SPDX-License-Identifier: GPL-2.0

//! xHCI Ring Buffer Management (Command, Event, and Transfer Rings).
//!
//! Handles 16-byte Transfer Request Blocks (TRBs), cycle bit toggling,
//! and DMA memory allocation for xHCI ring architectures.

use ostd::mm::{HasPaddr, dma::DmaCoherent, io::util::HasVmReaderWriter};
use crate::api::errno::{Errno, Result};

pub const TRB_SIZE: usize = 16;
pub const RING_ENTRIES: usize = 64; // 64 entries * 16 bytes = 1024 bytes (fits within 1 page)

pub const TRB_TYPE_NORMAL: u32 = 1;
pub const TRB_TYPE_SETUP_STAGE: u32 = 2;
pub const TRB_TYPE_DATA_STAGE: u32 = 3;
pub const TRB_TYPE_STATUS_STAGE: u32 = 4;
pub const TRB_TYPE_LINK: u32 = 6;
pub const TRB_TYPE_ENABLE_SLOT: u32 = 9;
pub const TRB_TYPE_DISABLE_SLOT: u32 = 10;
pub const TRB_TYPE_ADDRESS_DEVICE: u32 = 11;
pub const TRB_TYPE_CONFIGURE_ENDPOINT: u32 = 12;
pub const TRB_TYPE_EVALUATE_CONTEXT: u32 = 13;
pub const TRB_TYPE_RESET_ENDPOINT: u32 = 14;
pub const TRB_TYPE_TRANSFER_EVENT: u32 = 32;
pub const TRB_TYPE_COMMAND_COMPLETION: u32 = 33;
pub const TRB_TYPE_PORT_STATUS_CHANGE: u32 = 34;

/// Raw 16-byte Transfer Request Block (TRB).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Trb {
    pub parameter: u64,
    pub status: u32,
    pub control: u32,
}

impl Trb {
    pub fn new(parameter: u64, status: u32, control: u32) -> Self {
        Self {
            parameter,
            status,
            control,
        }
    }

    /// Extract TRB type (bits 10..=15 of control).
    pub fn trb_type(&self) -> u32 {
        (self.control >> 10) & 0x3F
    }

    /// Extract Cycle bit (bit 0 of control).
    pub fn cycle_bit(&self) -> bool {
        (self.control & 0x01) != 0
    }

    /// Completion code (bits 24..=31 of status).
    pub fn completion_code(&self) -> u8 {
        ((self.status >> 24) & 0xFF) as u8
    }

    /// Slot ID (bits 24..=31 of control).
    pub fn slot_id(&self) -> u8 {
        ((self.control >> 24) & 0xFF) as u8
    }
}

// ----------------------------------------------------------------------------
// Command Ring
// ----------------------------------------------------------------------------

/// Manages the Host Controller Command Ring.
pub struct CommandRing {
    dma: DmaCoherent,
    paddr: u64,
    enqueue_idx: usize,
    cycle_state: bool,
}

impl CommandRing {
    pub fn new() -> Result<Self> {
        let dma = DmaCoherent::alloc(1, true).map_err(|_| Errno::ENOMEM)?;
        let paddr = dma.paddr() as u64;

        let ring = Self {
            dma,
            paddr,
            enqueue_idx: 0,
            cycle_state: true,
        };

        // Write Link TRB at the end of the ring (index RING_ENTRIES - 1)
        // Link TRB points back to the ring base address with Toggle Cycle (bit 1) set.
        let link_ctrl = (TRB_TYPE_LINK << 10) | (1 << 1); // Toggle Cycle = 1
        ring.write_trb_at(RING_ENTRIES - 1, &Trb::new(paddr, 0, link_ctrl))?;

        Ok(ring)
    }

    pub fn paddr(&self) -> u64 {
        self.paddr
    }

    pub fn cycle_state(&self) -> bool {
        self.cycle_state
    }

    /// Enqueue a command TRB and update cycle state.
    pub fn enqueue(&mut self, parameter: u64, status: u32, control: u32) -> Result<u64> {
        let mut ctrl = control;
        if self.cycle_state {
            ctrl |= 0x01;
        } else {
            ctrl &= !0x01;
        }

        let trb_paddr = self.paddr + (self.enqueue_idx * TRB_SIZE) as u64;
        self.write_trb_at(self.enqueue_idx, &Trb::new(parameter, status, ctrl))?;

        self.enqueue_idx += 1;
        if self.enqueue_idx >= RING_ENTRIES - 1 {
            // Reached link TRB, toggle cycle state and wrap around
            let mut link_trb = self.read_trb_at(RING_ENTRIES - 1)?;
            if self.cycle_state {
                link_trb.control |= 0x01;
            } else {
                link_trb.control &= !0x01;
            }
            self.write_trb_at(RING_ENTRIES - 1, &link_trb)?;

            self.enqueue_idx = 0;
            self.cycle_state = !self.cycle_state;
        }

        Ok(trb_paddr)
    }

    fn write_trb_at(&self, idx: usize, trb: &Trb) -> Result<()> {
        let offset = idx * TRB_SIZE;
        let mut writer = self.dma.writer();
        writer.skip(offset);
        writer.write_val(&trb.parameter).map_err(|_| Errno::EIO)?;
        writer.write_val(&trb.status).map_err(|_| Errno::EIO)?;
        writer.write_val(&trb.control).map_err(|_| Errno::EIO)?;
        Ok(())
    }

    fn read_trb_at(&self, idx: usize) -> Result<Trb> {
        let offset = idx * TRB_SIZE;
        let mut reader = self.dma.reader();
        reader.skip(offset);
        let parameter = reader.read_val().map_err(|_| Errno::EIO)?;
        let status = reader.read_val().map_err(|_| Errno::EIO)?;
        let control = reader.read_val().map_err(|_| Errno::EIO)?;
        Ok(Trb { parameter, status, control })
    }
}

// ----------------------------------------------------------------------------
// Event Ring
// ----------------------------------------------------------------------------

/// Manages the Primary Event Ring and Event Ring Segment Table (ERST).
pub struct EventRing {
    ring_dma: DmaCoherent,
    erst_dma: DmaCoherent,
    pub ring_paddr: u64,
    pub erst_paddr: u64,
    dequeue_idx: usize,
    cycle_state: bool,
}

impl EventRing {
    pub fn new() -> Result<Self> {
        let ring_dma = DmaCoherent::alloc(1, true).map_err(|_| Errno::ENOMEM)?;
        let erst_dma = DmaCoherent::alloc(1, true).map_err(|_| Errno::ENOMEM)?;

        let ring_paddr = ring_dma.paddr() as u64;
        let erst_paddr = erst_dma.paddr() as u64;

        // Initialize ERST entry:
        // u64: Ring Segment Base Address
        // u16: Ring Segment Size (number of TRBs)
        // u16: Reserved
        // u32: Reserved
        let mut writer = erst_dma.writer();
        writer.write_val(&ring_paddr).map_err(|_| Errno::EIO)?;
        writer.write_val(&(RING_ENTRIES as u16)).map_err(|_| Errno::EIO)?;
        writer.write_val(&0u16).map_err(|_| Errno::EIO)?;
        writer.write_val(&0u32).map_err(|_| Errno::EIO)?;

        Ok(Self {
            ring_dma,
            erst_dma,
            ring_paddr,
            erst_paddr,
            dequeue_idx: 0,
            cycle_state: true,
        })
    }

    pub fn current_dequeue_ptr(&self) -> u64 {
        self.ring_paddr + (self.dequeue_idx * TRB_SIZE) as u64
    }

    /// Polls for the next pending event TRB.
    pub fn poll_event(&mut self) -> Result<Option<Trb>> {
        let offset = self.dequeue_idx * TRB_SIZE;
        let mut reader = self.ring_dma.reader();
        reader.skip(offset);
        let parameter = reader.read_val().map_err(|_| Errno::EIO)?;
        let status = reader.read_val().map_err(|_| Errno::EIO)?;
        let control: u32 = reader.read_val().map_err(|_| Errno::EIO)?;

        let trb_cycle = (control & 0x01) != 0;
        if trb_cycle != self.cycle_state {
            return Ok(None); // Hardware hasn't written this event yet
        }

        self.dequeue_idx += 1;
        if self.dequeue_idx >= RING_ENTRIES {
            self.dequeue_idx = 0;
            self.cycle_state = !self.cycle_state;
        }

        Ok(Some(Trb { parameter, status, control }))
    }
}

// ----------------------------------------------------------------------------
// Transfer Ring
// ----------------------------------------------------------------------------

/// Ring buffer for endpoint transfers (Control, Bulk, Interrupt).
pub struct TransferRing {
    dma: DmaCoherent,
    paddr: u64,
    enqueue_idx: usize,
    cycle_state: bool,
}

impl TransferRing {
    pub fn new() -> Result<Self> {
        let dma = DmaCoherent::alloc(1, true).map_err(|_| Errno::ENOMEM)?;
        let paddr = dma.paddr() as u64;

        let ring = Self {
            dma,
            paddr,
            enqueue_idx: 0,
            cycle_state: true,
        };

        // Link TRB at end of ring
        let link_ctrl = (TRB_TYPE_LINK << 10) | (1 << 1);
        ring.write_trb_at(RING_ENTRIES - 1, &Trb::new(paddr, 0, link_ctrl))?;

        Ok(ring)
    }

    pub fn paddr(&self) -> u64 {
        self.paddr
    }

    pub fn enqueue(&mut self, parameter: u64, status: u32, control: u32) -> Result<u64> {
        let mut ctrl = control;
        if self.cycle_state {
            ctrl |= 0x01;
        } else {
            ctrl &= !0x01;
        }

        let trb_paddr = self.paddr + (self.enqueue_idx * TRB_SIZE) as u64;
        self.write_trb_at(self.enqueue_idx, &Trb::new(parameter, status, ctrl))?;

        self.enqueue_idx += 1;
        if self.enqueue_idx >= RING_ENTRIES - 1 {
            let mut link_trb = self.read_trb_at(RING_ENTRIES - 1)?;
            if self.cycle_state {
                link_trb.control |= 0x01;
            } else {
                link_trb.control &= !0x01;
            }
            self.write_trb_at(RING_ENTRIES - 1, &link_trb)?;

            self.enqueue_idx = 0;
            self.cycle_state = !self.cycle_state;
        }

        Ok(trb_paddr)
    }

    fn write_trb_at(&self, idx: usize, trb: &Trb) -> Result<()> {
        let offset = idx * TRB_SIZE;
        let mut writer = self.dma.writer();
        writer.skip(offset);
        writer.write_val(&trb.parameter).map_err(|_| Errno::EIO)?;
        writer.write_val(&trb.status).map_err(|_| Errno::EIO)?;
        writer.write_val(&trb.control).map_err(|_| Errno::EIO)?;
        Ok(())
    }

    fn read_trb_at(&self, idx: usize) -> Result<Trb> {
        let offset = idx * TRB_SIZE;
        let mut reader = self.dma.reader();
        reader.skip(offset);
        let parameter = reader.read_val().map_err(|_| Errno::EIO)?;
        let status = reader.read_val().map_err(|_| Errno::EIO)?;
        let control = reader.read_val().map_err(|_| Errno::EIO)?;
        Ok(Trb { parameter, status, control })
    }
}
