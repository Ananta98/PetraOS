/// NVMe Submission and Completion Queue Management
///
/// Implements the 64-byte Submission Queue Entry (SQE) layout and the
/// 16-byte Completion Queue Entry (CQE) layout as defined by the NVM Express
/// specification, along with the queue state tracker used during command
/// dispatch and completion polling.
use ostd::mm::dma::DmaCoherent;
use ostd::mm::{VmIo, VmIoOnce};

use super::regs;

// ──────────────────────────────────────────────────────────────
// Submission Queue Entry layout (64 bytes)
// ──────────────────────────────────────────────────────────────

/// Byte offset of DW0 (opcode, FUSE, PSDT, CID) within an SQE.
pub const SQE_DW0: usize = 0;
/// Byte offset of DW1 (Namespace ID) within an SQE.
pub const SQE_NSID: usize = 4;
/// Byte offset of DW2-DW3 (reserved) within an SQE.
pub const SQE_RESERVED: usize = 8;
/// Byte offset of DW4-DW5 (Metadata Pointer) within an SQE.
pub const SQE_MPTR: usize = 16;
/// Byte offset of DW6-DW9 (Data Pointer — PRP1 and PRP2) within an SQE.
pub const SQE_PRP1: usize = 24;
pub const SQE_PRP2: usize = 32;
/// Byte offset of DW10 within an SQE (command-specific).
pub const SQE_CDW10: usize = 40;
/// Byte offset of DW11 within an SQE (command-specific).
pub const SQE_CDW11: usize = 44;
/// Byte offset of DW12 within an SQE (command-specific).
pub const SQE_CDW12: usize = 48;
/// Byte offset of DW13 within an SQE (command-specific).
pub const SQE_CDW13: usize = 52;
/// Byte offset of DW14 within an SQE (command-specific).
pub const SQE_CDW14: usize = 56;
/// Byte offset of DW15 within an SQE (command-specific).
pub const SQE_CDW15: usize = 60;
/// Size of a single Submission Queue Entry in bytes.
pub const SQE_SIZE: usize = 64;

// ──────────────────────────────────────────────────────────────
// Completion Queue Entry layout (16 bytes)
// ──────────────────────────────────────────────────────────────

/// Byte offset of DW0 (Command Specific) within a CQE.
pub const CQE_DW0: usize = 0;
/// Byte offset of DW1 (reserved) within a CQE.
pub const CQE_DW1: usize = 4;
/// Byte offset of DW2 (SQ Head Pointer + SQ Identifier) within a CQE.
pub const CQE_DW2: usize = 8;
/// Byte offset of DW3 (CID + Phase Tag + Status Field) within a CQE.
pub const CQE_DW3: usize = 12;
/// Size of a single Completion Queue Entry in bytes.
pub const CQE_SIZE: usize = 16;

// ──────────────────────────────────────────────────────────────
// Status field bit extraction helpers
// ──────────────────────────────────────────────────────────────

/// Extract the Status Code from CQE DW3 (bits [14:9]).
#[inline]
pub fn cqe_status_code(dw3: u16) -> u8 {
    ((dw3 >> 1) & 0xFF) as u8
}

/// Extract the Status Code Type from CQE DW3 (bits [11:9] of the upper byte).
#[inline]
pub fn cqe_status_code_type(dw3: u16) -> u8 {
    ((dw3 >> 9) & 0x07) as u8
}

// ──────────────────────────────────────────────────────────────
// Queue state tracker
// ──────────────────────────────────────────────────────────────

/// Tracks the state of a submission/completion queue pair.
///
/// The caller must keep this in sync with the doorbell registers after each
/// enqueue (SQ tail) and dequeue (CQ head) operation.
pub struct QueueState {
    /// Queue ID assigned to this queue pair (0 = Admin, ≥ 1 = IO).
    pub queue_id: u16,
    /// Current tail index into the Submission Queue.
    pub sq_tail: u16,
    /// Current head index into the Completion Queue.
    pub cq_head: u16,
    /// Expected Phase Tag for the next completion entry.
    pub phase: u16,
    /// Maximum number of entries in the SQ/CQ.
    pub capacity: u16,
}

impl QueueState {
    /// Create a new queue state at the beginning of both queues.
    pub fn new(queue_id: u16, capacity: u16) -> Self {
        Self {
            queue_id,
            sq_tail: 0,
            cq_head: 0,
            // Entries delivered in the first pass have phase bit = 1.
            phase: 1,
            capacity,
        }
    }

    /// Advance the SQ tail index, wrapping around at `capacity`.
    pub fn advance_sq_tail(&mut self) {
        self.sq_tail = (self.sq_tail + 1) % self.capacity;
    }

    /// Advance the CQ head index, wrapping around at `capacity`.
    ///
    /// When head wraps to 0 the expected Phase Tag toggles.
    pub fn advance_cq_head(&mut self) {
        self.cq_head = (self.cq_head + 1) % self.capacity;
        if self.cq_head == 0 {
            self.phase ^= 1;
        }
    }
}

// ──────────────────────────────────────────────────────────────
// SQE write helpers
// ──────────────────────────────────────────────────────────────

/// Write a 64-byte zeroed SQE at the given slot index into a DMA buffer
/// that holds the Submission Queue.
///
/// After zeroing, the caller may overwrite individual fields using
/// [`write_sqe_field_u32`] or [`write_sqe_field_u64`].
pub fn clear_sqe(sq_dma: &DmaCoherent, slot: u16) -> Result<(), ostd::Error> {
    let base = slot as usize * SQE_SIZE;
    let zeros = [0u8; SQE_SIZE];
    sq_dma.write_bytes(base, &zeros)
}

/// Write a 32-bit field inside the SQE at `slot`.
pub fn write_sqe_field_u32(
    sq_dma: &DmaCoherent,
    slot: u16,
    field_offset: usize,
    value: u32,
) -> Result<(), ostd::Error> {
    let offset = slot as usize * SQE_SIZE + field_offset;
    sq_dma.write_once(offset, &value)
}

/// Write a 64-bit field inside the SQE at `slot`.
pub fn write_sqe_field_u64(
    sq_dma: &DmaCoherent,
    slot: u16,
    field_offset: usize,
    value: u64,
) -> Result<(), ostd::Error> {
    let offset = slot as usize * SQE_SIZE + field_offset;
    // NVMe SQEs are little-endian; write low word then high word.
    sq_dma.write_once(offset, &(value as u32))?;
    sq_dma.write_once(offset + 4, &((value >> 32) as u32))
}

// ──────────────────────────────────────────────────────────────
// CQE read helpers
// ──────────────────────────────────────────────────────────────

/// Read a 32-bit field from the CQE at `slot` in a DMA buffer.
pub fn read_cqe_field_u32(
    cq_dma: &DmaCoherent,
    slot: u16,
    field_offset: usize,
) -> Result<u32, ostd::Error> {
    let offset = slot as usize * CQE_SIZE + field_offset;
    cq_dma.read_once(offset)
}

/// Poll the completion queue until the entry at the current CQ head position
/// carries the expected phase tag.
///
/// Returns `Ok(dw0)` with the command-specific result word on success, or
/// `Err` when the controller signals a non-zero status code.
pub fn poll_completion(
    cq_dma: &DmaCoherent,
    state: &mut QueueState,
    mmio: &ostd::io::IoMem,
) -> Result<u32, ostd::Error> {
    loop {
        // DW3 holds [status | phase_tag] in the upper 16 bits of the 16-byte CQE.
        let dw3_raw: u32 = read_cqe_field_u32(cq_dma, state.cq_head, CQE_DW3)?;
        let dw3 = (dw3_raw & 0xFFFF) as u16; // lower 16 bits = {status[14:1], phase[0]}
        let phase = dw3 & regs::CQE_PHASE_BIT;

        // Phase tag matches → completion arrived.
        if phase == state.phase {
            let status = cqe_status_code(dw3 >> 1); // SC field
            // Read the command-specific result before advancing.
            let dw0 = read_cqe_field_u32(cq_dma, state.cq_head, CQE_DW0)?;

            // Advance CQ head and ring its doorbell.
            state.advance_cq_head();
            mmio.write_once(
                regs::cq_head_doorbell(state.queue_id),
                &(state.cq_head as u32),
            )?;

            if status != 0 {
                return Err(ostd::Error::IoError);
            }
            return Ok(dw0);
        }

        // No entry yet — spin until the controller posts a completion.
        core::hint::spin_loop();
    }
}
