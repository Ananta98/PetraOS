/// NVMe Controller Register Offsets and Bit Definitions
///
/// All offsets are relative to the NVMe controller's BAR0 MMIO base.
/// Register layout follows the NVM Express 1.4 specification.

// ──────────────────────────────────────────────────────────────
// Controller Capabilities and Version
// ──────────────────────────────────────────────────────────────

/// Controller Capabilities (CAP) — 64-bit register at offset 0x00.
pub const REG_CAP: usize = 0x00;
/// Version (VS) — 32-bit register at offset 0x08.
pub const REG_VS: usize = 0x08;
/// Interrupt Mask Set (INTMS) — 32-bit register at offset 0x0C.
pub const REG_INTMS: usize = 0x0C;
/// Interrupt Mask Clear (INTMC) — 32-bit register at offset 0x10.
pub const REG_INTMC: usize = 0x10;

// ──────────────────────────────────────────────────────────────
// Controller Configuration and Status
// ──────────────────────────────────────────────────────────────

/// Controller Configuration (CC) — 32-bit register at offset 0x14.
pub const REG_CC: usize = 0x14;
/// Controller Status (CSTS) — 32-bit register at offset 0x1C.
pub const REG_CSTS: usize = 0x1C;
/// NVM Subsystem Reset (NSSR) — 32-bit register at offset 0x20.
pub const REG_NSSR: usize = 0x20;
/// Admin Queue Attributes (AQA) — 32-bit register at offset 0x24.
pub const REG_AQA: usize = 0x24;
/// Admin Submission Queue Base Address (ASQ) — 64-bit at offset 0x28.
pub const REG_ASQ: usize = 0x28;
/// Admin Completion Queue Base Address (ACQ) — 64-bit at offset 0x30.
pub const REG_ACQ: usize = 0x30;

// ──────────────────────────────────────────────────────────────
// Doorbell base — stride is 2^(2 + DSTRD) bytes per doorbell pair
// ──────────────────────────────────────────────────────────────

/// Base offset of the first doorbell register (Admin SQ tail doorbell).
pub const REG_DOORBELL_BASE: usize = 0x1000;

// ──────────────────────────────────────────────────────────────
// Controller Configuration (CC) bit fields
// ──────────────────────────────────────────────────────────────

/// CC.EN — Enable the controller.  Setting 1 → enabled; 0 → disabled.
pub const CC_EN: u32 = 1 << 0;
/// CC.CSS — Command Set Selected; 0x00 = NVM command set (bits [6:4]).
pub const CC_CSS_NVM: u32 = 0b000 << 4;
/// CC.MPS — Memory Page Size; 0 = 4 KiB (bits [10:7]).
pub const CC_MPS_4K: u32 = 0 << 7;
/// CC.AMS — Arbitration Mechanism Selected; 0 = round-robin (bits [13:11]).
pub const CC_AMS_RR: u32 = 0 << 11;
/// CC.IOSQES — IO Submission Queue Entry Size = 64 bytes (2^6), bits [19:16].
pub const CC_IOSQES: u32 = 6 << 16;
/// CC.IOCQES — IO Completion Queue Entry Size = 16 bytes (2^4), bits [23:20].
pub const CC_IOCQES: u32 = 4 << 20;

// ──────────────────────────────────────────────────────────────
// Controller Status (CSTS) bit fields
// ──────────────────────────────────────────────────────────────

/// CSTS.RDY — Controller is ready.
pub const CSTS_RDY: u32 = 1 << 0;
/// CSTS.CFS — Controller Fatal Status.
pub const CSTS_CFS: u32 = 1 << 1;

// ──────────────────────────────────────────────────────────────
// Queue sizes (entries minus 1 stored in AQA)
// ──────────────────────────────────────────────────────────────

/// Number of entries in the Admin Submission Queue (must be power-of-2, ≤ 4096).
pub const ADMIN_QUEUE_SIZE: u32 = 64;
/// Number of entries in the Admin Completion Queue.
pub const ADMIN_CQ_SIZE: u32 = 64;
/// Number of entries in the IO Submission Queue.
pub const IO_QUEUE_SIZE: u32 = 256;
/// Number of entries in the IO Completion Queue.
pub const IO_CQ_SIZE: u32 = 256;

// ──────────────────────────────────────────────────────────────
// NVM Express Admin Command Opcodes (Admin Command Set)
// ──────────────────────────────────────────────────────────────

/// Delete I/O Submission Queue admin command opcode.
pub const ADMIN_OPCODE_DELETE_IO_SQ: u8 = 0x00;
/// Create I/O Submission Queue admin command opcode.
pub const ADMIN_OPCODE_CREATE_IO_SQ: u8 = 0x01;
/// Delete I/O Completion Queue admin command opcode.
pub const ADMIN_OPCODE_DELETE_IO_CQ: u8 = 0x04;
/// Create I/O Completion Queue admin command opcode.
pub const ADMIN_OPCODE_CREATE_IO_CQ: u8 = 0x05;
/// Identify admin command opcode — retrieve controller/namespace data.
pub const ADMIN_OPCODE_IDENTIFY: u8 = 0x06;
/// Abort admin command opcode.
pub const ADMIN_OPCODE_ABORT: u8 = 0x08;
/// Set Features admin command opcode.
pub const ADMIN_OPCODE_SET_FEATURES: u8 = 0x09;
/// Get Features admin command opcode.
pub const ADMIN_OPCODE_GET_FEATURES: u8 = 0x0A;

// ──────────────────────────────────────────────────────────────
// NVM Express I/O Command Opcodes (NVM Command Set)
// ──────────────────────────────────────────────────────────────

/// NVM Flush command opcode.
pub const NVM_OPCODE_FLUSH: u8 = 0x00;
/// NVM Write command opcode.
pub const NVM_OPCODE_WRITE: u8 = 0x01;
/// NVM Read command opcode.
pub const NVM_OPCODE_READ: u8 = 0x02;

// ──────────────────────────────────────────────────────────────
// Identify CNS (Controller or Namespace Structure) values
// ──────────────────────────────────────────────────────────────

/// Identify Namespace data structure.
pub const IDENTIFY_CNS_NAMESPACE: u32 = 0x00;
/// Identify Controller data structure.
pub const IDENTIFY_CNS_CONTROLLER: u32 = 0x01;
/// Identify Active Namespace ID list.
pub const IDENTIFY_CNS_NS_LIST: u32 = 0x02;

// ──────────────────────────────────────────────────────────────
// Completion Queue entry status/phase bit
// ──────────────────────────────────────────────────────────────

/// Phase tag bit in the completion queue entry DW3.  Toggled by the controller
/// on each full pass through the queue to signal new completions.
pub const CQE_PHASE_BIT: u16 = 1 << 0;

// ──────────────────────────────────────────────────────────────
// Default NVMe namespace identifier
// ──────────────────────────────────────────────────────────────

/// Default namespace ID used for all I/O commands (namespace 1).
pub const DEFAULT_NAMESPACE_ID: u32 = 1;

// ──────────────────────────────────────────────────────────────
// Logical block size constant
// ──────────────────────────────────────────────────────────────

/// The standard NVMe logical block size in bytes (512 B).
pub const NVME_BLOCK_SIZE: usize = 512;

/// Compute the Submission Queue Tail Doorbell offset for a given queue ID.
///
/// Each queue pair uses two consecutive 4-byte doorbell registers.
/// The stride between doorbell pairs is fixed at 4 bytes (DSTRD = 0 in CAP).
#[inline]
pub fn sq_tail_doorbell(queue_id: u16) -> usize {
    REG_DOORBELL_BASE + (queue_id as usize) * 8
}

/// Compute the Completion Queue Head Doorbell offset for a given queue ID.
#[inline]
pub fn cq_head_doorbell(queue_id: u16) -> usize {
    REG_DOORBELL_BASE + (queue_id as usize) * 8 + 4
}
