//! Physical Frame Allocator (`buddy_system_allocator::FrameAllocator`).
//!
//! Manages 4 KiB physical page frames as frame numbers (`phys >> PAGE_SHIFT`).
//! Per-frame state (usable/allocated, buddy order, COW reference count) lives
//! in the bump-allocated [`PageFrameMetadata`] array; free-block membership
//! lives in the [`FrameAllocator`] index sets.
//!
//! Re-entrancy note: [`FrameAllocator`] keeps its index in `BTreeSet`s, whose
//! nodes allocate from the kernel global allocator — and that allocator grows
//! from these very frames (`slab`). Every mutating entry point below therefore
//! holds a [`FrameAllocGuard`], which routes small in-flight allocations to a
//! dedicated static early heap; frees are routed back by address, so pairing
//! stays correct under any interleaving. See `slab.rs` for the protocol.

use super::slab::FrameAllocGuard;
use crate::mm::PhysAddr;
use bitflags::bitflags;
use buddy_system_allocator::FrameAllocator;

pub const PAGE_SIZE: u64 = 4096;
pub const PAGE_SHIFT: u64 = 12;

/// Number of buddy orders. The largest block holds `2^(ORDERS - 1)` frames
/// (`24` orders → 8 Mi frames = 32 GiB maximum single block).
pub const FRAME_BUDDY_ORDERS: usize = 24;

bitflags! {
    /// Flags representing physical page frame state.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
    pub struct PageFrameFlags: u16 {
        /// Page is in usable RAM identified by the bootloader.
        const USABLE    = 1 << 0;
        /// Page is currently allocated to virtual memory, kernel, or slab.
        const ALLOCATED = 1 << 1;
        /// Page is reserved (kernel image, early metadata, firmware).
        const RESERVED  = 1 << 2;
    }
}

/// Metadata tracked for each 4 KiB physical frame in the system.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct PageFrameMetadata {
    pub ref_count: u32,
    pub flags: PageFrameFlags,
    pub order: u8,
}

impl PageFrameMetadata {
    pub const fn new() -> Self {
        Self {
            ref_count: 0,
            flags: PageFrameFlags::empty(),
            order: 0,
        }
    }

    #[inline(always)]
    pub fn is_usable(&self) -> bool {
        self.flags.contains(PageFrameFlags::USABLE)
    }

    #[inline(always)]
    pub fn is_allocated(&self) -> bool {
        self.flags.contains(PageFrameFlags::ALLOCATED)
    }

    #[inline(always)]
    pub fn is_reserved(&self) -> bool {
        self.flags.contains(PageFrameFlags::RESERVED)
    }

    #[inline(always)]
    pub fn set_allocated(&mut self, order: u8) {
        self.flags.insert(PageFrameFlags::ALLOCATED);
        self.order = order;
        self.ref_count = 1;
    }

    #[inline(always)]
    pub fn clear_allocated(&mut self) {
        self.flags.remove(PageFrameFlags::ALLOCATED);
        self.order = 0;
        self.ref_count = 0;
    }

    #[inline(always)]
    pub fn inc_ref(&mut self) -> u32 {
        self.ref_count = self.ref_count.saturating_add(1);
        self.ref_count
    }

    #[inline(always)]
    pub fn dec_ref(&mut self) -> u32 {
        if self.ref_count > 0 {
            self.ref_count -= 1;
        }
        self.ref_count
    }

    #[inline(always)]
    pub fn get_ref(&self) -> u32 {
        self.ref_count
    }
}

#[inline(always)]
pub fn phys_to_page_idx(paddr: PhysAddr) -> usize {
    (paddr.as_u64() >> PAGE_SHIFT) as usize
}

#[inline(always)]
pub fn page_idx_to_phys(idx: usize) -> PhysAddr {
    PhysAddr::new((idx as u64) << PAGE_SHIFT)
}

/// Physical memory manager utilizing the buddy-system frame allocator.
pub struct BuddyFrameAllocator {
    frames: FrameAllocator<FRAME_BUDDY_ORDERS>,
    metadata: Option<&'static mut [PageFrameMetadata]>,
    total_pages: usize,
    allocated_pages: usize,
}

unsafe impl Send for BuddyFrameAllocator {}
unsafe impl Sync for BuddyFrameAllocator {}

impl BuddyFrameAllocator {
    /// Creates an uninitialized buddy frame allocator instance.
    pub const fn new() -> Self {
        Self {
            frames: FrameAllocator::new(),
            metadata: None,
            total_pages: 0,
            allocated_pages: 0,
        }
    }

    /// Initializes the buddy allocator with available memory map entries
    /// and the pre-allocated page metadata slice.
    ///
    /// Returns the usable page count. The caller logs it after releasing the
    /// allocator lock so boot logging never nests inside the frame critical
    /// section.
    pub fn init(
        &mut self,
        metadata: &'static mut [PageFrameMetadata],
        metadata_phys_start: u64,
        metadata_phys_end: u64,
    ) -> usize {
        // SAFETY: Routes index-set traffic to the early heap (see slab.rs).
        let _guard = FrameAllocGuard::enter();
        self.metadata = Some(metadata);
        self.frames = FrameAllocator::new();
        self.total_pages = 0;
        self.allocated_pages = 0;

        let memmap_response = crate::limine::MEMORY_MAP_REQUEST
            .get_response()
            .expect("BuddyAllocator init: Limine memory map response is missing");

        for entry in memmap_response.entries() {
            if entry.entry_type != limine::memory_map::EntryType::USABLE {
                continue;
            }

            // Exclude sub-1MB low memory to prevent clobbering BIOS/EBDA/IVT
            let region_start = entry.base.max(0x100_000);
            let aligned_start = (region_start + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
            let aligned_end = (entry.base + entry.length) & !(PAGE_SIZE - 1);

            if aligned_end <= aligned_start {
                continue;
            }

            // Split around metadata allocation range
            if aligned_start < metadata_phys_end && aligned_end > metadata_phys_start {
                // Segment 1: before metadata
                if aligned_start < metadata_phys_start {
                    self.add_usable_range(aligned_start, metadata_phys_start);
                }
                // Segment 2: after metadata
                if aligned_end > metadata_phys_end {
                    self.add_usable_range(metadata_phys_end, aligned_end);
                }
            } else {
                self.add_usable_range(aligned_start, aligned_end);
            }
        }

        self.total_pages
    }

    /// Registers `[start_phys, end_phys)` as free frames.
    fn add_usable_range(&mut self, start_phys: u64, end_phys: u64) {
        let meta = match self.metadata {
            Some(ref mut m) => m,
            None => return,
        };

        let start_idx = phys_to_page_idx(PhysAddr::new(start_phys));
        let end_idx = phys_to_page_idx(PhysAddr::new(end_phys)).min(meta.len());
        if start_idx >= end_idx {
            return;
        }

        for i in start_idx..end_idx {
            meta[i].flags.insert(PageFrameFlags::USABLE);
        }
        self.total_pages += end_idx - start_idx;
        self.frames.add_frame(start_idx, end_idx);
    }

    /// Allocate `2^order` contiguous, size-aligned physical pages.
    pub fn alloc_pages(&mut self, order: usize) -> Option<PhysAddr> {
        if order >= FRAME_BUDDY_ORDERS {
            return None;
        }
        // SAFETY: Routes index-set traffic to the early heap (see slab.rs).
        let _guard = FrameAllocGuard::enter();

        let meta = match self.metadata {
            Some(ref mut m) => m,
            None => return None,
        };

        let count = 1usize << order;
        let head = self.frames.alloc(count)?;

        if head >= meta.len() {
            self.frames.dealloc(head, count);
            return None;
        }
        meta[head].set_allocated(order as u8);
        for i in 1..count {
            if head + i < meta.len() {
                meta[head + i].flags.insert(PageFrameFlags::ALLOCATED);
            }
        }

        self.allocated_pages += count;
        Some(page_idx_to_phys(head))
    }

    /// Allocate a single 4 KiB physical page (order 0).
    pub fn alloc_page(&mut self) -> Option<PhysAddr> {
        self.alloc_pages(0)
    }

    /// Free a block of `2^order` physical pages back to the buddy allocator.
    pub fn free_pages(&mut self, paddr: PhysAddr, order: usize) {
        if !paddr.is_aligned(PAGE_SIZE) {
            log::error!(
                "free_pages: address {:#x} is not page-aligned",
                paddr.as_u64()
            );
            return;
        }

        if paddr.as_u64() < 0x1000 {
            log::warn!(
                "free_pages: refusing to free sub-page-0 address {:#x}",
                paddr.as_u64()
            );
            return;
        }

        let head_idx = phys_to_page_idx(paddr);
        let meta = match self.metadata {
            Some(ref mut m) => m,
            None => return,
        };

        if head_idx >= meta.len() {
            log::error!(
                "free_pages: page index {} out of bounds for {:#x}",
                head_idx,
                paddr.as_u64()
            );
            return;
        }

        if !meta[head_idx].is_usable() {
            log::warn!(
                "free_pages: attempting to free non-usable page at {:#x}",
                paddr.as_u64()
            );
            return;
        }

        if !meta[head_idx].is_allocated() {
            log::warn!("free_pages: double free detected at {:#x}", paddr.as_u64());
            return;
        }

        let alloc_order = meta[head_idx].order as usize;
        let actual_order = if order != alloc_order {
            log::warn!(
                "free_pages: order mismatch at {:#x}: requested {}, recorded in metadata {}. Using recorded order.",
                paddr.as_u64(),
                order,
                alloc_order
            );
            alloc_order
        } else {
            order
        };

        if actual_order >= FRAME_BUDDY_ORDERS {
            log::error!(
                "free_pages: recorded order {} out of range at {:#x}",
                actual_order,
                paddr.as_u64()
            );
            return;
        }

        let new_ref = meta[head_idx].dec_ref();
        if new_ref > 0 {
            // Page is still referenced by another shared mapping (e.g. COW)
            return;
        }

        let count = 1usize << actual_order;
        if head_idx + count > meta.len() {
            log::error!(
                "free_pages: block {:#x} order {} exceeds metadata bounds",
                paddr.as_u64(),
                actual_order
            );
            return;
        }
        for i in 0..count {
            meta[head_idx + i].clear_allocated();
        }
        self.allocated_pages = self.allocated_pages.saturating_sub(count);

        // SAFETY: Routes index-set traffic to the early heap (see slab.rs).
        let _guard = FrameAllocGuard::enter();
        self.frames.dealloc(head_idx, count);
    }

    /// Free a single 4 KiB physical page (order 0).
    pub fn free_page(&mut self, paddr: PhysAddr) {
        self.free_pages(paddr, 0);
    }

    /// Increment reference count for a physical page frame.
    pub fn inc_ref(&mut self, paddr: PhysAddr) {
        let idx = phys_to_page_idx(paddr);
        if let Some(ref mut meta) = self.metadata {
            if idx < meta.len() {
                meta[idx].inc_ref();
            }
        }
    }

    /// Decrement reference count for a physical page frame without returning to buddy pool.
    pub fn dec_ref(&mut self, paddr: PhysAddr) -> u32 {
        let idx = phys_to_page_idx(paddr);
        if let Some(ref mut meta) = self.metadata {
            if idx < meta.len() {
                meta[idx].dec_ref()
            } else {
                0
            }
        } else {
            0
        }
    }

    /// Query reference count for a physical page frame.
    pub fn get_ref(&self, paddr: PhysAddr) -> u32 {
        let idx = phys_to_page_idx(paddr);
        if let Some(ref meta) = self.metadata {
            if idx < meta.len() {
                meta[idx].get_ref()
            } else {
                0
            }
        } else {
            0
        }
    }

    /// Returns the total number of usable pages managed by the buddy allocator.
    pub fn total_pages(&self) -> usize {
        self.total_pages
    }

    /// Returns the approximate number of free pages in the system.
    pub fn free_pages_count(&self) -> usize {
        self.total_pages.saturating_sub(self.allocated_pages)
    }
}
