/// VirtIO Split Virtqueue Implementation
///
/// Implements the VirtIO 1.x split virtqueue model used for communication
/// between the driver (guest) and the device (host). A virtqueue consists
/// of three areas in physically-contiguous DMA memory:
///
/// 1. **Descriptor Table** — array of `VirtqDesc` entries describing buffers
/// 2. **Available Ring** — ring of descriptor chain heads made available to the device
/// 3. **Used Ring** — ring of completed descriptor chains returned by the device
///
/// This module manages descriptor allocation via a simple free-list and
/// provides `submit_request` / `poll_used` for synchronous I/O.
use ostd::io::IoMem;
use ostd::mm::dma::DmaCoherent;
use ostd::mm::{HasDaddr, VmIo, VmIoOnce};

use super::regs;

// ──────────────────────────────────────────────────────────────
// Virtqueue descriptor flags
// ──────────────────────────────────────────────────────────────

/// This descriptor continues via the `next` field.
const VIRTQ_DESC_F_NEXT: u16 = 1;

/// Buffer is device-writable (i.e., the device writes into it).
const VIRTQ_DESC_F_WRITE: u16 = 2;

// ──────────────────────────────────────────────────────────────
// Virtqueue layout sizes
// ──────────────────────────────────────────────────────────────

/// Size of one virtqueue descriptor entry (16 bytes).
///
/// Layout:
/// - addr  (u64) — physical address of the buffer
/// - len   (u32) — length of the buffer in bytes
/// - flags (u16) — VIRTQ_DESC_F_NEXT, VIRTQ_DESC_F_WRITE, etc.
/// - next  (u16) — index of next descriptor in chain
const DESC_SIZE: usize = 16;

/// Size of the available ring header (flags + idx = 4 bytes).
const AVAIL_RING_HEADER: usize = 4;

/// Size of the used ring header (flags + idx = 4 bytes).
const USED_RING_HEADER: usize = 4;

/// Size of one used ring element (id: u32 + len: u32 = 8 bytes).
const USED_ELEM_SIZE: usize = 8;

// ──────────────────────────────────────────────────────────────
// VirtqueueLayout — pre-computed offsets for a given queue size
// ──────────────────────────────────────────────────────────────

/// Pre-computed byte offsets within the DMA region for the three
/// virtqueue areas. All offsets are relative to the DMA region base.
#[derive(Debug, Clone, Copy)]
pub struct VirtqueueLayout {
    /// Offset of the descriptor table (always 0).
    pub desc_offset: usize,
    /// Offset of the available ring.
    pub avail_offset: usize,
    /// Offset of the used ring.
    pub used_offset: usize,
    /// Total size in bytes needed for this virtqueue.
    pub total_size: usize,
}

impl VirtqueueLayout {
    /// Compute the layout for a virtqueue of `queue_size` entries.
    ///
    /// The available ring is placed immediately after the descriptor table,
    /// and the used ring is page-aligned (4096) after the available ring
    /// per the VirtIO specification.
    pub fn compute(queue_size: u16) -> Self {
        let n = queue_size as usize;

        // Descriptor table: n * 16 bytes
        let desc_size = n * DESC_SIZE;

        // Available ring: 4 (header) + 2*n (ring entries) + 2 (used_event)
        let avail_size = AVAIL_RING_HEADER + 2 * n + 2;

        // Used ring must be page-aligned
        let avail_end = desc_size + avail_size;
        let used_offset = (avail_end + 0xFFF) & !0xFFF;

        // Used ring: 4 (header) + 8*n (used elements) + 2 (avail_event)
        let used_size = USED_RING_HEADER + USED_ELEM_SIZE * n + 2;

        Self {
            desc_offset: 0,
            avail_offset: desc_size,
            used_offset,
            total_size: used_offset + used_size,
        }
    }
}

// ──────────────────────────────────────────────────────────────
// VirtqueueState — mutable queue state tracking
// ──────────────────────────────────────────────────────────────

/// Tracks the mutable state of a single split virtqueue.
pub struct VirtqueueState {
    /// Number of entries in the queue.
    pub queue_size: u16,
    /// Pre-computed layout offsets.
    pub layout: VirtqueueLayout,
    /// Index of the next free descriptor in the free list.
    /// `u16::MAX` means the free list is exhausted.
    free_head: u16,
    /// Next index to write into the available ring.
    avail_idx: u16,
    /// Last used ring index we have processed.
    last_used_idx: u16,
}

impl VirtqueueState {
    /// Create a new `VirtqueueState` for a queue of `queue_size` entries.
    ///
    /// Initializes the free list so that descriptor 0 → 1 → 2 → … → (n-1),
    /// with descriptor (n-1) pointing to `u16::MAX` (end of list).
    pub fn new(queue_size: u16) -> Self {
        Self {
            queue_size,
            layout: VirtqueueLayout::compute(queue_size),
            free_head: 0,
            avail_idx: 0,
            last_used_idx: 0,
        }
    }

    /// Initialize the descriptor free-list in the DMA region.
    ///
    /// Each descriptor's `next` field points to the following descriptor,
    /// forming a singly-linked free list. The last descriptor has
    /// `next = 0` and `flags = 0` (no NEXT flag).
    pub fn init_free_list(&self, dma: &DmaCoherent) -> Result<(), ostd::Error> {
        for i in 0..self.queue_size {
            let desc_base = self.layout.desc_offset + (i as usize) * DESC_SIZE;
            // addr = 0
            dma.write_bytes(desc_base, &0u64.to_le_bytes())?;
            // len = 0
            dma.write_bytes(desc_base + 8, &0u32.to_le_bytes())?;

            if i + 1 < self.queue_size {
                // flags = NEXT
                dma.write_bytes(desc_base + 12, &VIRTQ_DESC_F_NEXT.to_le_bytes())?;
                // next = i + 1
                dma.write_bytes(desc_base + 14, &(i + 1).to_le_bytes())?;
            } else {
                // Last descriptor: no next
                dma.write_bytes(desc_base + 12, &0u16.to_le_bytes())?;
                dma.write_bytes(desc_base + 14, &0u16.to_le_bytes())?;
            }
        }
        Ok(())
    }

    /// Allocate a single descriptor from the free list.
    ///
    /// Returns the descriptor index, or an error if the free list is empty.
    fn alloc_descriptor(&mut self, dma: &DmaCoherent) -> Result<u16, ostd::Error> {
        if self.free_head >= self.queue_size {
            return Err(ostd::Error::NotEnoughResources);
        }

        let idx = self.free_head;

        // Read the `next` field to advance the free list head
        let desc_base = self.layout.desc_offset + (idx as usize) * DESC_SIZE;
        let mut next_bytes = [0u8; 2];
        dma.read_bytes(desc_base + 14, &mut next_bytes)?;
        let next = u16::from_le_bytes(next_bytes);

        // Read flags to check if there is a next pointer
        let mut flags_bytes = [0u8; 2];
        dma.read_bytes(desc_base + 12, &mut flags_bytes)?;
        let flags = u16::from_le_bytes(flags_bytes);

        if (flags & VIRTQ_DESC_F_NEXT) != 0 {
            self.free_head = next;
        } else {
            // This was the last free descriptor
            self.free_head = self.queue_size; // sentinel: exhausted
        }

        Ok(idx)
    }

    /// Return a descriptor to the free list.
    fn free_descriptor(&mut self, dma: &DmaCoherent, idx: u16) -> Result<(), ostd::Error> {
        let desc_base = self.layout.desc_offset + (idx as usize) * DESC_SIZE;

        if self.free_head < self.queue_size {
            // Point this descriptor's next to the current free head
            dma.write_bytes(desc_base + 12, &VIRTQ_DESC_F_NEXT.to_le_bytes())?;
            dma.write_bytes(desc_base + 14, &self.free_head.to_le_bytes())?;
        } else {
            // Free list was empty; this becomes the only entry
            dma.write_bytes(desc_base + 12, &0u16.to_le_bytes())?;
            dma.write_bytes(desc_base + 14, &0u16.to_le_bytes())?;
        }
        self.free_head = idx;

        Ok(())
    }

    /// Submit a virtio-blk request as a 3-descriptor chain.
    ///
    /// The chain consists of:
    /// 1. **Header descriptor** (device-readable): `virtio_blk_req` header
    ///    containing type, reserved, and sector fields.
    /// 2. **Data descriptor** (device-readable for writes, device-writable
    ///    for reads): the actual data buffer.
    /// 3. **Status descriptor** (device-writable): 1-byte status response.
    ///
    /// After building the chain, the head descriptor index is placed in the
    /// available ring and the device is notified via the legacy queue notify
    /// register.
    ///
    /// # Arguments
    /// * `dma` — The DMA-coherent region holding the virtqueue structures
    /// * `header_buf` — DMA buffer containing the pre-built request header
    /// * `data_buf` — DMA buffer for the data transfer
    /// * `status_buf` — DMA buffer for the 1-byte status response
    /// * `data_len` — Length of the data buffer in bytes
    /// * `is_write` — `true` for write requests (data is device-readable)
    /// * `io_bar` — The legacy I/O BAR for queue notification
    pub fn submit_request(
        &mut self,
        dma: &DmaCoherent,
        header_buf: &DmaCoherent,
        data_buf: &DmaCoherent,
        status_buf: &DmaCoherent,
        data_len: usize,
        is_write: bool,
        io_bar: &IoMem,
    ) -> Result<u16, ostd::Error> {
        // Allocate 3 descriptors
        let desc_header = self.alloc_descriptor(dma)?;
        let desc_data = match self.alloc_descriptor(dma) {
            Ok(d) => d,
            Err(e) => {
                self.free_descriptor(dma, desc_header)?;
                return Err(e);
            }
        };
        let desc_status = match self.alloc_descriptor(dma) {
            Ok(d) => d,
            Err(e) => {
                self.free_descriptor(dma, desc_data)?;
                self.free_descriptor(dma, desc_header)?;
                return Err(e);
            }
        };

        // ── Descriptor 0: Header (device-readable) ──────────────
        let base0 = self.layout.desc_offset + (desc_header as usize) * DESC_SIZE;
        let header_phys = header_buf.daddr() as u64;
        dma.write_bytes(base0, &header_phys.to_le_bytes())?;
        dma.write_bytes(base0 + 8, &(regs::BLK_REQ_HEADER_SIZE as u32).to_le_bytes())?;
        dma.write_bytes(base0 + 12, &VIRTQ_DESC_F_NEXT.to_le_bytes())?;
        dma.write_bytes(base0 + 14, &desc_data.to_le_bytes())?;

        // ── Descriptor 1: Data buffer ───────────────────────────
        let base1 = self.layout.desc_offset + (desc_data as usize) * DESC_SIZE;
        let data_phys = data_buf.daddr() as u64;
        dma.write_bytes(base1, &data_phys.to_le_bytes())?;
        dma.write_bytes(base1 + 8, &(data_len as u32).to_le_bytes())?;

        let data_flags = if is_write {
            // Write: device reads from this buffer → device-readable, chain continues
            VIRTQ_DESC_F_NEXT
        } else {
            // Read: device writes into this buffer → device-writable, chain continues
            VIRTQ_DESC_F_WRITE | VIRTQ_DESC_F_NEXT
        };
        dma.write_bytes(base1 + 12, &data_flags.to_le_bytes())?;
        dma.write_bytes(base1 + 14, &desc_status.to_le_bytes())?;

        // ── Descriptor 2: Status byte (device-writable) ─────────
        let base2 = self.layout.desc_offset + (desc_status as usize) * DESC_SIZE;
        let status_phys = status_buf.daddr() as u64;
        dma.write_bytes(base2, &status_phys.to_le_bytes())?;
        dma.write_bytes(base2 + 8, &1u32.to_le_bytes())?; // 1 byte
        dma.write_bytes(base2 + 12, &VIRTQ_DESC_F_WRITE.to_le_bytes())?;
        dma.write_bytes(base2 + 14, &0u16.to_le_bytes())?; // no next

        // ── Add head to available ring ──────────────────────────
        let avail_base = self.layout.avail_offset;
        let ring_idx = (self.avail_idx % self.queue_size) as usize;

        // avail->ring[ring_idx] = desc_header
        dma.write_bytes(
            avail_base + AVAIL_RING_HEADER + ring_idx * 2,
            &desc_header.to_le_bytes(),
        )?;

        // Memory barrier: ensure descriptor writes are visible before idx update
        core::sync::atomic::fence(core::sync::atomic::Ordering::Release);

        // Increment avail->idx
        self.avail_idx = self.avail_idx.wrapping_add(1);
        dma.write_bytes(avail_base + 2, &self.avail_idx.to_le_bytes())?;

        // Memory barrier: ensure idx is visible before notification
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

        // ── Notify the device (legacy: write queue index to notify register) ──
        io_bar.write_once(regs::LEGACY_QUEUE_NOTIFY, &0u16)?;

        Ok(desc_header)
    }

    /// Poll the used ring until the device returns a completed request.
    ///
    /// Spins until `used->idx` advances past `last_used_idx`, then reads
    /// the used element to retrieve the head descriptor index. Returns
    /// the 1-byte status code from the status buffer.
    ///
    /// After processing, the 3-descriptor chain is returned to the free list.
    pub fn poll_used(
        &mut self,
        dma: &DmaCoherent,
        status_buf: &DmaCoherent,
        head_desc: u16,
    ) -> Result<u8, ostd::Error> {
        let used_base = self.layout.used_offset;

        // Spin until the device increments used->idx
        loop {
            let mut idx_bytes = [0u8; 2];
            dma.read_bytes(used_base + 2, &mut idx_bytes)?;
            let used_idx = u16::from_le_bytes(idx_bytes);

            if used_idx != self.last_used_idx {
                break;
            }

            core::hint::spin_loop();
        }

        // Read the used element at last_used_idx
        let elem_offset = used_base
            + USED_RING_HEADER
            + (self.last_used_idx % self.queue_size) as usize * USED_ELEM_SIZE;

        let mut id_bytes = [0u8; 4];
        dma.read_bytes(elem_offset, &mut id_bytes)?;
        let _used_id = u32::from_le_bytes(id_bytes);

        self.last_used_idx = self.last_used_idx.wrapping_add(1);

        // Read the status byte from the status buffer
        let mut status = [0u8; 1];
        status_buf.read_bytes(0, &mut status)?;

        // Free the 3-descriptor chain (walk from head)
        // We know the chain is exactly: head_desc → data → status
        // Read data desc index from head's next field
        let head_base = self.layout.desc_offset + (head_desc as usize) * DESC_SIZE;
        let mut next_bytes = [0u8; 2];
        dma.read_bytes(head_base + 14, &mut next_bytes)?;
        let data_desc = u16::from_le_bytes(next_bytes);

        let data_base = self.layout.desc_offset + (data_desc as usize) * DESC_SIZE;
        dma.read_bytes(data_base + 14, &mut next_bytes)?;
        let status_desc = u16::from_le_bytes(next_bytes);

        // Free in reverse order
        self.free_descriptor(dma, status_desc)?;
        self.free_descriptor(dma, data_desc)?;
        self.free_descriptor(dma, head_desc)?;

        Ok(status[0])
    }
}
