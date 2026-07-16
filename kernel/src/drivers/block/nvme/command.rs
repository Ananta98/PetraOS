/// NVMe Admin and I/O Command Construction
///
/// Provides functions to build and submit Admin commands (Identify, Create
/// Queue) and NVM I/O commands (Read, Write) into the controller's Admin
/// Submission Queue and the IO Submission Queue respectively.
///
/// All commands use Physical Region Page (PRP) data transfer, with a single
/// PRP1 entry pointing to a DMA-coherent buffer.
use ostd::io::IoMem;
use ostd::mm::dma::DmaCoherent;
use ostd::mm::{HasDaddr, VmIo, VmIoOnce};

use super::{queue, regs};

// ──────────────────────────────────────────────────────────────
// Identify Command
// ──────────────────────────────────────────────────────────────

/// Submit an Identify command to the Admin Queue and poll for completion.
///
/// On success, the 4096-byte response is copied into `out_buf`.
///
/// # Arguments
/// - `cns` — Controller or Namespace Structure selector (see [`regs`]).
/// - `namespace_id` — Namespace ID (0 for controller-level identify).
pub fn identify(
    mmio: &IoMem,
    admin_sq: &DmaCoherent,
    admin_cq: &DmaCoherent,
    data_buf: &DmaCoherent,
    state: &mut queue::QueueState,
    cns: u32,
    namespace_id: u32,
    command_id: u16,
    out_buf: &mut [u8],
) -> Result<(), ostd::Error> {
    let slot = state.sq_tail;

    queue::clear_sqe(admin_sq, slot)?;

    // DW0: opcode, command ID
    let dw0 = (regs::ADMIN_OPCODE_IDENTIFY as u32) | ((command_id as u32) << 16);
    queue::write_sqe_field_u32(admin_sq, slot, queue::SQE_DW0, dw0)?;

    // DW1: Namespace ID
    queue::write_sqe_field_u32(admin_sq, slot, queue::SQE_NSID, namespace_id)?;

    // DW6-7: PRP1 — physical address of the 4096-byte identity buffer
    let prp1 = data_buf.daddr() as u64;
    queue::write_sqe_field_u64(admin_sq, slot, queue::SQE_PRP1, prp1)?;

    // DW10: CNS field
    queue::write_sqe_field_u32(admin_sq, slot, queue::SQE_CDW10, cns)?;

    // Advance SQ tail and ring the SQ tail doorbell
    state.advance_sq_tail();
    mmio.write_once(
        regs::sq_tail_doorbell(state.queue_id),
        &(state.sq_tail as u32),
    )?;

    // Wait for completion
    queue::poll_completion(admin_cq, state, mmio)?;

    // Copy result out of the DMA buffer
    data_buf.read_bytes(0, out_buf)
}

// ──────────────────────────────────────────────────────────────
// Create I/O Completion Queue
// ──────────────────────────────────────────────────────────────

/// Submit a Create I/O Completion Queue command and poll for completion.
///
/// Allocates a physically-contiguous completion queue backed by `cq_dma`.
///
/// # Arguments
/// - `io_cq_id` — Queue identifier to assign (≥ 1 for IO queues).
/// - `queue_size` — Number of entries in the completion queue (≤ 65535).
pub fn create_io_cq(
    mmio: &IoMem,
    admin_sq: &DmaCoherent,
    admin_cq: &DmaCoherent,
    cq_dma: &DmaCoherent,
    state: &mut queue::QueueState,
    io_cq_id: u16,
    queue_size: u16,
    command_id: u16,
) -> Result<(), ostd::Error> {
    let slot = state.sq_tail;

    queue::clear_sqe(admin_sq, slot)?;

    // DW0: Create IO CQ opcode
    let dw0 = (regs::ADMIN_OPCODE_CREATE_IO_CQ as u32) | ((command_id as u32) << 16);
    queue::write_sqe_field_u32(admin_sq, slot, queue::SQE_DW0, dw0)?;

    // DW6-7: PRP1 — base address of the CQ buffer
    let prp1 = cq_dma.daddr() as u64;
    queue::write_sqe_field_u64(admin_sq, slot, queue::SQE_PRP1, prp1)?;

    // DW10: Queue ID [15:0] | Queue Size [31:16] (size is 0-based, i.e. entries - 1)
    let cdw10 = (io_cq_id as u32) | (((queue_size - 1) as u32) << 16);
    queue::write_sqe_field_u32(admin_sq, slot, queue::SQE_CDW10, cdw10)?;

    // DW11: Physically contiguous (PC bit 0) | interrupts disabled
    queue::write_sqe_field_u32(admin_sq, slot, queue::SQE_CDW11, 0x0001)?;

    state.advance_sq_tail();
    mmio.write_once(
        regs::sq_tail_doorbell(state.queue_id),
        &(state.sq_tail as u32),
    )?;

    queue::poll_completion(admin_cq, state, mmio)?;
    Ok(())
}

// ──────────────────────────────────────────────────────────────
// Create I/O Submission Queue
// ──────────────────────────────────────────────────────────────

/// Submit a Create I/O Submission Queue command and poll for completion.
///
/// # Arguments
/// - `io_sq_id` — Queue identifier (must match a previously created IO CQ).
/// - `io_cq_id` — Completion queue ID to pair this SQ with.
/// - `queue_size` — Number of entries in the submission queue.
pub fn create_io_sq(
    mmio: &IoMem,
    admin_sq: &DmaCoherent,
    admin_cq: &DmaCoherent,
    sq_dma: &DmaCoherent,
    state: &mut queue::QueueState,
    io_sq_id: u16,
    io_cq_id: u16,
    queue_size: u16,
    command_id: u16,
) -> Result<(), ostd::Error> {
    let slot = state.sq_tail;

    queue::clear_sqe(admin_sq, slot)?;

    let dw0 = (regs::ADMIN_OPCODE_CREATE_IO_SQ as u32) | ((command_id as u32) << 16);
    queue::write_sqe_field_u32(admin_sq, slot, queue::SQE_DW0, dw0)?;

    let prp1 = sq_dma.daddr() as u64;
    queue::write_sqe_field_u64(admin_sq, slot, queue::SQE_PRP1, prp1)?;

    // DW10: Queue ID | Queue Size (0-based)
    let cdw10 = (io_sq_id as u32) | (((queue_size - 1) as u32) << 16);
    queue::write_sqe_field_u32(admin_sq, slot, queue::SQE_CDW10, cdw10)?;

    // DW11: PC bit set | associated CQ ID in bits [31:16]
    let cdw11 = 0x0001u32 | ((io_cq_id as u32) << 16);
    queue::write_sqe_field_u32(admin_sq, slot, queue::SQE_CDW11, cdw11)?;

    state.advance_sq_tail();
    mmio.write_once(
        regs::sq_tail_doorbell(state.queue_id),
        &(state.sq_tail as u32),
    )?;

    queue::poll_completion(admin_cq, state, mmio)?;
    Ok(())
}

// ──────────────────────────────────────────────────────────────
// I/O Read / Write Commands
// ──────────────────────────────────────────────────────────────

/// Submit an NVM Read or Write command to the IO queue and poll for completion.
///
/// A single PRP1 entry is used for transfers up to one page (4 KiB).
/// For 512-byte LBAs this covers up to 8 sectors in a single command.
///
/// # Arguments
/// - `io_sq`/`io_cq` — DMA buffers backing the IO submission/completion queues.
/// - `io_state` — Queue state for the IO queue pair (queue ID ≥ 1).
/// - `write` — `true` for Write, `false` for Read.
/// - `lba` — Starting logical block address.
/// - `block_count` — Number of logical blocks (0-based in CDW12, so `blocks - 1`).
pub fn submit_io(
    mmio: &IoMem,
    io_sq: &DmaCoherent,
    io_cq: &DmaCoherent,
    dma_buf: &DmaCoherent,
    io_state: &mut queue::QueueState,
    namespace_id: u32,
    write: bool,
    lba: u64,
    block_count: u16,
    command_id: u16,
) -> Result<(), ostd::Error> {
    let slot = io_state.sq_tail;
    let opcode = if write {
        regs::NVM_OPCODE_WRITE
    } else {
        regs::NVM_OPCODE_READ
    };

    queue::clear_sqe(io_sq, slot)?;

    // DW0: opcode + command ID
    let dw0 = (opcode as u32) | ((command_id as u32) << 16);
    queue::write_sqe_field_u32(io_sq, slot, queue::SQE_DW0, dw0)?;

    // DW1: Namespace ID
    queue::write_sqe_field_u32(io_sq, slot, queue::SQE_NSID, namespace_id)?;

    // DW6-7: PRP1 — physical address of the data buffer
    let prp1 = dma_buf.daddr() as u64;
    queue::write_sqe_field_u64(io_sq, slot, queue::SQE_PRP1, prp1)?;

    // DW10-11: Starting LBA (64-bit split across two 32-bit fields)
    queue::write_sqe_field_u32(io_sq, slot, queue::SQE_CDW10, lba as u32)?;
    queue::write_sqe_field_u32(io_sq, slot, queue::SQE_CDW11, (lba >> 32) as u32)?;

    // DW12: Number of logical blocks (0-based, so block_count - 1)
    queue::write_sqe_field_u32(io_sq, slot, queue::SQE_CDW12, (block_count - 1) as u32)?;

    // Advance SQ tail and ring doorbell
    io_state.advance_sq_tail();
    mmio.write_once(
        regs::sq_tail_doorbell(io_state.queue_id),
        &(io_state.sq_tail as u32),
    )?;

    // Wait for the IO completion
    queue::poll_completion(io_cq, io_state, mmio)?;
    Ok(())
}
