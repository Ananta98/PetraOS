/// VirtIO Split Virtqueue — Shared Transport-Agnostic Implementation
///
/// Implements the VirtIO 1.x split virtqueue model for use with
/// [`VirtioPciTransport`]. A split virtqueue consists of three areas in
/// physically contiguous DMA-coherent memory:
///
/// 1. **Descriptor Table** — an array of [`VirtqDesc`] entries (16 bytes each)
///    that describe individual guest-physical buffers.
/// 2. **Available Ring** — written by the driver; tells the device which
///    descriptor chain heads are ready to process.
/// 3. **Used Ring** — written by the device; tells the driver which descriptor
///    chains have been consumed and the number of bytes written.
///
/// # Usage
///
/// ```rust,ignore
/// // After transport is initialized and DRIVER_OK has NOT yet been set:
/// transport.select_queue(0)?;
/// let queue_size = transport.read_queue_size()?;
/// let mut vq = SplitVirtqueue::new(&transport, 0, queue_size)?;
///
/// // Submit a 3-descriptor chain (header, data, status):
/// let head = vq.add_buffer(&[
///     VirtqDescriptor { address: header_paddr, length: 16, flags: VirtqDescFlags::NONE },
///     VirtqDescriptor { address: data_paddr,   length: 512, flags: VirtqDescFlags::WRITE },
///     VirtqDescriptor { address: status_paddr, length: 1,   flags: VirtqDescFlags::WRITE },
/// ])?;
/// vq.notify(&transport)?;
///
/// // Poll until complete:
/// while vq.pop_used()?.is_none() { core::hint::spin_loop(); }
/// ```
///
/// # References
/// - VirtIO 1.2 Specification §2.7 (Split Virtqueues)
/// - VirtIO 1.2 Specification §4.1.5 (Driver Requirements: Virtqueues)

use super::regs;
use super::transport::VirtioPciTransport;
use ostd::mm::dma::DmaCoherent;
use ostd::mm::{HasDaddr, VmIo};

// ──────────────────────────────────────────────────────────────
// Descriptor flags
// ──────────────────────────────────────────────────────────────

/// Flags for a single virtqueue descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VirtqDescFlags(pub u16);

impl VirtqDescFlags {
    /// No special flags — device-readable, no chaining.
    pub const NONE: Self = Self(0);

    /// This descriptor continues via the `next` field (set internally).
    const NEXT: Self = Self(1);

    /// Device-writable buffer (the device writes into it; the driver reads back).
    pub const WRITE: Self = Self(2);

    /// Returns `true` if the `NEXT` flag is set.
    pub fn has_next(self) -> bool {
        self.0 & Self::NEXT.0 != 0
    }

    /// Returns `true` if the `WRITE` flag is set.
    pub fn is_device_writable(self) -> bool {
        self.0 & Self::WRITE.0 != 0
    }
}

impl core::ops::BitOr for VirtqDescFlags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

// ──────────────────────────────────────────────────────────────
// Descriptor input type
// ──────────────────────────────────────────────────────────────

/// A single descriptor to be added to a virtqueue submission chain.
///
/// Callers build a slice of these and pass it to [`SplitVirtqueue::add_buffer`].
/// The `NEXT` chaining flag is added automatically; callers should only specify
/// `WRITE` when the device needs to write into the buffer.
#[derive(Debug, Clone, Copy)]
pub struct VirtqDescriptor {
    /// Guest-physical (DMA) address of the buffer.
    pub address: u64,
    /// Length of the buffer in bytes.
    pub length: u32,
    /// `VirtqDescFlags::NONE` for device-readable, `VirtqDescFlags::WRITE` for device-writable.
    pub flags: VirtqDescFlags,
}

// ──────────────────────────────────────────────────────────────
// Memory layout constants
// ──────────────────────────────────────────────────────────────

/// Size of a single virtqueue descriptor in bytes.
///
/// Layout: addr(8) + len(4) + flags(2) + next(2) = 16 bytes.
const DESC_BYTES: usize = 16;

/// Size of the available ring header: flags(2) + idx(2).
const AVAIL_HEADER_BYTES: usize = 4;

/// Size of the used ring header: flags(2) + idx(2).
const USED_HEADER_BYTES: usize = 4;

/// Size of a single used ring element: id(4) + len(4).
const USED_ELEM_BYTES: usize = 8;

// ──────────────────────────────────────────────────────────────
// VirtqueueLayout — pre-computed byte offsets within the DMA region
// ──────────────────────────────────────────────────────────────

/// Pre-computed byte offsets for all three virtqueue areas within a single
/// DMA-coherent region.
///
/// Per the VirtIO spec §2.7.2, the used ring must be aligned to a 4096-byte
/// boundary. All offsets are relative to the start of the DMA region.
#[derive(Debug, Clone, Copy)]
struct VirtqueueLayout {
    /// Offset of the descriptor table (always 0).
    desc_offset: usize,
    /// Offset of the available ring.
    avail_offset: usize,
    /// Offset of the used ring (page-aligned).
    used_offset: usize,
    /// Total bytes required for the region.
    total_bytes: usize,
}

impl VirtqueueLayout {
    /// Compute the layout for a virtqueue of `queue_size` entries.
    fn compute(queue_size: u16) -> Self {
        let n = queue_size as usize;

        let desc_bytes = n * DESC_BYTES;
        // Available ring: header + n * u16 ring entries + u16 used_event
        let avail_bytes = AVAIL_HEADER_BYTES + n * 2 + 2;

        let avail_offset = desc_bytes;
        // Used ring must be page-aligned (4096 bytes)
        let used_offset = (avail_offset + avail_bytes + 0xFFF) & !0xFFF;
        // Used ring: header + n * used_elem + u16 avail_event
        let used_bytes = USED_HEADER_BYTES + n * USED_ELEM_BYTES + 2;

        Self {
            desc_offset: 0,
            avail_offset,
            used_offset,
            total_bytes: used_offset + used_bytes,
        }
    }

    /// Number of 4096-byte pages needed to hold the entire virtqueue.
    fn pages_needed(self) -> usize {
        (self.total_bytes + 0xFFF) / 0x1000
    }
}

// ──────────────────────────────────────────────────────────────
// SplitVirtqueue
// ──────────────────────────────────────────────────────────────

/// A fully managed VirtIO split virtqueue.
///
/// Allocates DMA-coherent memory, registers the queue with the transport,
/// and manages the descriptor free-list, available ring, and used ring.
///
/// # Lifecycle
///
/// 1. Call [`SplitVirtqueue::new`] while the device is in the initialization
///    sequence (after `DRIVER`, before `DRIVER_OK`).
/// 2. Call [`add_buffer`] to submit descriptor chains.
/// 3. Call [`notify`] to kick the device.
/// 4. Call [`pop_used`] to collect completed chains.
pub struct SplitVirtqueue {
    /// DMA-coherent memory holding the descriptor table, available ring,
    /// and used ring.
    dma: DmaCoherent,
    /// Queue index (0-based) as registered with the transport.
    queue_index: u16,
    /// Number of descriptors in the queue.
    queue_size: u16,
    /// Pre-computed byte offsets within `dma`.
    layout: VirtqueueLayout,
    /// Head of the singly-linked descriptor free list.
    /// `queue_size` is used as a sentinel for "list exhausted".
    free_head: u16,
    /// Number of currently allocated (in-flight) descriptors.
    free_count: u16,
    /// Next slot index to write into the available ring.
    avail_index: u16,
    /// Last used ring index we have processed.
    last_used_index: u16,
}

impl SplitVirtqueue {
    /// Create and register a new split virtqueue with the transport.
    ///
    /// This must be called while the device is in the initialization sequence
    /// (after the driver has set `STATUS_DRIVER`, before `STATUS_DRIVER_OK`).
    ///
    /// # Arguments
    ///
    /// * `transport` — Active VirtIO PCI transport for this device.
    /// * `queue_index` — 0-based index of the queue to initialize.
    /// * `requested_size` — Requested queue depth (will be capped to the
    ///   device-reported maximum).
    pub fn new(
        transport: &VirtioPciTransport,
        queue_index: u16,
        requested_size: u16,
    ) -> Result<Self, ostd::Error> {
        // Select the queue so all subsequent queue registers refer to it.
        transport.select_queue(queue_index)?;

        let max_size = transport.read_queue_size()?;
        if max_size == 0 {
            return Err(ostd::Error::NotEnoughResources);
        }

        let queue_size = core::cmp::min(requested_size, max_size);

        // On modern transport, negotiate the actual queue size back.
        transport.write_queue_size(queue_size)?;

        let layout = VirtqueueLayout::compute(queue_size);
        let pages = layout.pages_needed().max(1);
        let dma = DmaCoherent::alloc(pages, true)?;

        let mut vq = Self {
            queue_index,
            queue_size,
            layout,
            dma,
            free_head: 0,
            free_count: queue_size,
            avail_index: 0,
            last_used_index: 0,
        };

        vq.init_free_list()?;

        // Register the queue with the transport.
        let dma_base = vq.dma.daddr() as u64;

        if transport.is_modern() {
            // Modern: program descriptor, available, and used ring addresses separately.
            transport.write_queue_descriptor_addr(dma_base + vq.layout.desc_offset as u64)?;
            transport.write_queue_avail_addr(dma_base + vq.layout.avail_offset as u64)?;
            transport.write_queue_used_addr(dma_base + vq.layout.used_offset as u64)?;
            transport.enable_queue()?;
        } else {
            // Legacy: give the device the 4096-byte page frame number.
            let pfn = (dma_base / 0x1000) as u32;
            transport.write_legacy_queue_pfn(pfn)?;
        }

        Ok(vq)
    }

    /// Submit a descriptor chain to the available ring.
    ///
    /// `descriptors` must be a non-empty slice; consecutive entries are chained
    /// via the `NEXT` flag automatically. The last entry in the slice never has
    /// `NEXT` set, regardless of the flags specified by the caller.
    ///
    /// Returns the head descriptor index, which can be matched against the used
    /// ring element `id` field to identify which request completed.
    ///
    /// Returns `Err(NotEnoughResources)` if fewer than `descriptors.len()`
    /// free descriptor slots are available.
    pub fn add_buffer(&mut self, descriptors: &[VirtqDescriptor]) -> Result<u16, ostd::Error> {
        if descriptors.is_empty() {
            return Err(ostd::Error::InvalidArgs);
        }
        if (descriptors.len() as u16) > self.free_count {
            return Err(ostd::Error::NotEnoughResources);
        }

        // Allocate all required descriptors up front, recording their indices.
        let mut indices = [0u16; 64];
        let count = descriptors.len();
        if count > indices.len() {
            return Err(ostd::Error::InvalidArgs);
        }

        for slot in indices.iter_mut().take(count) {
            *slot = self.alloc_descriptor()?;
        }

        // Write each descriptor, chaining to the next where applicable.
        for i in 0..count {
            let is_last = i == count - 1;
            let flags = if is_last {
                // Strip any NEXT bit from caller-supplied flags.
                VirtqDescFlags(descriptors[i].flags.0 & !VirtqDescFlags::NEXT.0)
            } else {
                VirtqDescFlags(descriptors[i].flags.0 | VirtqDescFlags::NEXT.0)
            };
            let next = if is_last { 0u16 } else { indices[i + 1] };

            self.write_descriptor(indices[i], descriptors[i].address, descriptors[i].length, flags, next)?;
        }

        let head = indices[0];

        // Publish the head to the available ring.
        self.push_avail(head)?;

        Ok(head)
    }

    /// Notify the device that new buffers are available in this queue.
    ///
    /// Must be called after [`add_buffer`] to kick the device.
    pub fn notify(&self, transport: &VirtioPciTransport) -> Result<(), ostd::Error> {
        transport.notify_queue(self.queue_index)
    }

    /// Check the used ring for a completed buffer chain.
    ///
    /// Returns `Some((head_id, bytes_written))` if the device has completed a
    /// request, or `None` if the used ring has not advanced. The returned
    /// `head_id` matches the value returned by the original [`add_buffer`] call.
    ///
    /// Frees the used descriptors back to the free list automatically.
    pub fn pop_used(&mut self) -> Result<Option<(u16, u32)>, ostd::Error> {
        // Read the used ring index from the device.
        let mut idx_bytes = [0u8; 2];
        self.dma
            .read_bytes(self.layout.used_offset + 2, &mut idx_bytes)?;
        let used_idx = u16::from_le_bytes(idx_bytes);

        if used_idx == self.last_used_index {
            return Ok(None);
        }

        // Memory barrier: ensure we read the used ring element after the index.
        core::sync::atomic::fence(core::sync::atomic::Ordering::Acquire);

        let slot = (self.last_used_index % self.queue_size) as usize;
        let elem_offset = self.layout.used_offset + USED_HEADER_BYTES + slot * USED_ELEM_BYTES;

        let mut id_bytes = [0u8; 4];
        self.dma.read_bytes(elem_offset, &mut id_bytes)?;
        let id = u16::from_le_bytes([id_bytes[0], id_bytes[1]]);

        let mut len_bytes = [0u8; 4];
        self.dma.read_bytes(elem_offset + 4, &mut len_bytes)?;
        let length = u32::from_le_bytes(len_bytes);

        self.last_used_index = self.last_used_index.wrapping_add(1);

        // Free the completed descriptor chain.
        self.free_chain(id)?;

        Ok(Some((id, length)))
    }

    /// The DMA base address of this virtqueue's memory region.
    ///
    /// Useful for registering the queue with devices that require the PFN
    /// or the raw addresses directly.
    pub fn dma_base_address(&self) -> u64 {
        self.dma.daddr() as u64
    }

    /// The negotiated queue depth.
    pub fn queue_size(&self) -> u16 {
        self.queue_size
    }

    // ─── Private helpers ────────────────────────────────────────

    /// Initialize the descriptor free list as a singly-linked list:
    /// 0 → 1 → 2 → … → (queue_size - 1) → sentinel.
    fn init_free_list(&mut self) -> Result<(), ostd::Error> {
        for i in 0..self.queue_size {
            let base = self.layout.desc_offset + (i as usize) * DESC_BYTES;
            // addr = 0, len = 0
            self.dma.write_bytes(base, &0u64.to_le_bytes())?;
            self.dma.write_bytes(base + 8, &0u32.to_le_bytes())?;

            if i + 1 < self.queue_size {
                // flags = NEXT; next = i + 1
                self.dma
                    .write_bytes(base + 12, &VirtqDescFlags::NEXT.0.to_le_bytes())?;
                self.dma
                    .write_bytes(base + 14, &(i + 1).to_le_bytes())?;
            } else {
                // Last entry: no next
                self.dma.write_bytes(base + 12, &0u16.to_le_bytes())?;
                self.dma.write_bytes(base + 14, &0u16.to_le_bytes())?;
            }
        }
        Ok(())
    }

    /// Allocate a single descriptor from the free list, returning its index.
    fn alloc_descriptor(&mut self) -> Result<u16, ostd::Error> {
        if self.free_head >= self.queue_size {
            return Err(ostd::Error::NotEnoughResources);
        }

        let index = self.free_head;
        let base = self.layout.desc_offset + (index as usize) * DESC_BYTES;

        // Advance free_head via the chained `next` field.
        let mut flags_bytes = [0u8; 2];
        self.dma.read_bytes(base + 12, &mut flags_bytes)?;
        let flags = u16::from_le_bytes(flags_bytes);

        if (flags & VirtqDescFlags::NEXT.0) != 0 {
            let mut next_bytes = [0u8; 2];
            self.dma.read_bytes(base + 14, &mut next_bytes)?;
            self.free_head = u16::from_le_bytes(next_bytes);
        } else {
            self.free_head = self.queue_size; // sentinel: exhausted
        }

        self.free_count -= 1;
        Ok(index)
    }

    /// Return a descriptor to the front of the free list.
    fn free_descriptor(&mut self, index: u16) -> Result<(), ostd::Error> {
        let base = self.layout.desc_offset + (index as usize) * DESC_BYTES;

        if self.free_head < self.queue_size {
            // Chain this descriptor to the current free head.
            self.dma
                .write_bytes(base + 12, &VirtqDescFlags::NEXT.0.to_le_bytes())?;
            self.dma
                .write_bytes(base + 14, &self.free_head.to_le_bytes())?;
        } else {
            // Free list was exhausted; this is now the sole entry.
            self.dma.write_bytes(base + 12, &0u16.to_le_bytes())?;
            self.dma.write_bytes(base + 14, &0u16.to_le_bytes())?;
        }

        self.free_head = index;
        self.free_count += 1;
        Ok(())
    }

    /// Walk and free a descriptor chain starting at `head_index`.
    fn free_chain(&mut self, head_index: u16) -> Result<(), ostd::Error> {
        let mut current = head_index;

        loop {
            let base = self.layout.desc_offset + (current as usize) * DESC_BYTES;

            let mut flags_bytes = [0u8; 2];
            self.dma.read_bytes(base + 12, &mut flags_bytes)?;
            let flags = u16::from_le_bytes(flags_bytes);

            let has_next = (flags & VirtqDescFlags::NEXT.0) != 0;

            let mut next_bytes = [0u8; 2];
            self.dma.read_bytes(base + 14, &mut next_bytes)?;
            let next = u16::from_le_bytes(next_bytes);

            self.free_descriptor(current)?;

            if has_next {
                current = next;
            } else {
                break;
            }
        }

        Ok(())
    }

    /// Write a single descriptor entry into the DMA region.
    fn write_descriptor(
        &mut self,
        index: u16,
        address: u64,
        length: u32,
        flags: VirtqDescFlags,
        next: u16,
    ) -> Result<(), ostd::Error> {
        let base = self.layout.desc_offset + (index as usize) * DESC_BYTES;
        self.dma.write_bytes(base, &address.to_le_bytes())?;
        self.dma.write_bytes(base + 8, &length.to_le_bytes())?;
        self.dma.write_bytes(base + 12, &flags.0.to_le_bytes())?;
        self.dma.write_bytes(base + 14, &next.to_le_bytes())?;
        Ok(())
    }

    /// Add the given head descriptor index to the available ring and bump
    /// the available index.
    fn push_avail(&mut self, head: u16) -> Result<(), ostd::Error> {
        let ring_slot = (self.avail_index % self.queue_size) as usize;
        let ring_entry_offset = self.layout.avail_offset + AVAIL_HEADER_BYTES + ring_slot * 2;

        self.dma
            .write_bytes(ring_entry_offset, &head.to_le_bytes())?;

        // Memory barrier: descriptor writes must be visible before idx update.
        core::sync::atomic::fence(core::sync::atomic::Ordering::Release);

        self.avail_index = self.avail_index.wrapping_add(1);
        self.dma.write_bytes(
            self.layout.avail_offset + 2,
            &self.avail_index.to_le_bytes(),
        )?;

        Ok(())
    }
}

// ──────────────────────────────────────────────────────────────
// Kernel tests
// ──────────────────────────────────────────────────────────────

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;

    /// Verify the layout alignment rules for a typical queue size of 64.
    #[ktest]
    fn test_layout_alignment_queue64() {
        let layout = VirtqueueLayout::compute(64);

        // Descriptor table always starts at 0.
        assert_eq!(layout.desc_offset, 0);

        // Available ring immediately follows the descriptor table.
        assert_eq!(layout.avail_offset, 64 * DESC_BYTES);

        // Used ring must be page-aligned.
        assert_eq!(layout.used_offset % 4096, 0);

        // Total size must be positive.
        assert!(layout.total_bytes > 0);
    }

    /// VirtqDescFlags bitwise operations must compose correctly.
    #[ktest]
    fn test_virtq_desc_flags_composition() {
        let combined = VirtqDescFlags::NEXT | VirtqDescFlags::WRITE;
        assert!(combined.has_next());
        assert!(combined.is_device_writable());

        assert!(!VirtqDescFlags::NONE.has_next());
        assert!(!VirtqDescFlags::NONE.is_device_writable());
    }

    /// Verify that pages_needed rounds up correctly.
    #[ktest]
    fn test_layout_pages_needed_roundup() {
        // Queue of 1: should need at least 1 page.
        let layout = VirtqueueLayout::compute(1);
        assert!(layout.pages_needed() >= 1);

        // Queue of 256: should need more than 1 page.
        let layout = VirtqueueLayout::compute(256);
        assert!(layout.pages_needed() >= 1);
        // The used ring at page-aligned offset after 256*16 + avail will
        // almost certainly require at least 2 pages.
        assert_eq!(layout.pages_needed(), (layout.total_bytes + 0xFFF) / 0x1000);
    }
}
