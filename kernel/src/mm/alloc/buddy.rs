//! Physical Frame Allocator using a Metadata-Indexed Binary Buddy System.
//!
//! Manages 4 KiB physical page frames using a binary buddy allocation algorithm.
//! Free blocks are tracked using doubly-linked free lists with nodes stored
//! as 32-bit frame indices directly in the pre-carved `PageFrameMetadata` array.
//!
//! Features zero external crate dependencies, zero heap allocation overhead,
//! and complete immunity to physical RAM clobbering or circular re-entrancy.
//!
//! @author Ananta <kusumaananta042@gmail.com>

use super::free_list::{FreeListArray, FreeListNode, NO_FRAME};
use crate::mm::PhysAddr;
use bitflags::bitflags;

pub const PAGE_SIZE: u64 = 4096;
pub const PAGE_SHIFT: u64 = 12;

/// Number of buddy orders supported. Order `k` manages blocks of `2^k` contiguous pages.
/// With 21 orders, Order 20 represents 2^20 pages (4 GiB), covering full system memory.
pub const FRAME_BUDDY_ORDERS: usize = 21;

bitflags! {
    /// Flags representing physical page frame state.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
    pub struct PageFrameFlags: u16 {
        /// Page is in usable RAM identified by the bootloader.
        const USABLE    = 1 << 0;
        /// Page is currently allocated.
        const ALLOCATED = 1 << 1;
        /// Page is reserved (kernel image, metadata array, firmware).
        const RESERVED  = 1 << 2;
        /// Page is part of a multi-page allocation or buddy block, but not the head frame.
        const TAIL      = 1 << 3;
    }
}

/// Metadata tracked for each 4 KiB physical frame in the system.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct PageFrameMetadata {
    pub ref_count: u32,
    pub flags: PageFrameFlags,
    pub order: u8,
    pub next_free: u32,
    pub prev_free: u32,
}

impl PageFrameMetadata {
    pub const fn new() -> Self {
        Self {
            ref_count: 0,
            flags: PageFrameFlags::empty(),
            order: 0,
            next_free: NO_FRAME,
            prev_free: NO_FRAME,
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
    pub fn is_tail(&self) -> bool {
        self.flags.contains(PageFrameFlags::TAIL)
    }

    #[inline(always)]
    pub fn set_allocated(&mut self, order: u8) {
        self.flags.insert(PageFrameFlags::ALLOCATED);
        self.flags.remove(PageFrameFlags::TAIL);
        self.order = order;
        self.ref_count = 1;
        self.next_free = NO_FRAME;
        self.prev_free = NO_FRAME;
    }

    #[inline(always)]
    pub fn clear_allocated(&mut self) {
        self.flags
            .remove(PageFrameFlags::ALLOCATED | PageFrameFlags::TAIL);
        self.order = 0;
        self.ref_count = 0;
        self.next_free = NO_FRAME;
        self.prev_free = NO_FRAME;
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

impl FreeListNode for PageFrameMetadata {
    #[inline(always)]
    fn next_link(&self) -> u32 {
        self.next_free
    }

    #[inline(always)]
    fn prev_link(&self) -> u32 {
        self.prev_free
    }

    #[inline(always)]
    fn set_next_link(&mut self, next: u32) {
        self.next_free = next;
    }

    #[inline(always)]
    fn set_prev_link(&mut self, prev: u32) {
        self.prev_free = prev;
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

/// Physical memory manager utilizing a metadata-indexed binary buddy system.
pub struct BuddyFrameAllocator {
    metadata: Option<&'static mut [PageFrameMetadata]>,
    free_lists: FreeListArray<FRAME_BUDDY_ORDERS>,
    hhdm: u64,
    total_pages: usize,
    allocated_pages: usize,
}

unsafe impl Send for BuddyFrameAllocator {}
unsafe impl Sync for BuddyFrameAllocator {}

impl BuddyFrameAllocator {
    /// Creates an uninitialized buddy frame allocator instance.
    pub const fn new() -> Self {
        Self {
            metadata: None,
            free_lists: FreeListArray::new(),
            hhdm: 0,
            total_pages: 0,
            allocated_pages: 0,
        }
    }

    /// Initializes the buddy allocator directly from the bootloader memory map.
    /// Carves the `PageFrameMetadata` array from early usable RAM and establishes free lists.
    ///
    /// Returns the total count of usable 4 KiB pages managed.
    pub fn init(&mut self, hhdm_offset: u64) -> usize {
        self.hhdm = hhdm_offset;
        self.free_lists.clear();
        self.total_pages = 0;
        self.allocated_pages = 0;

        let memmap_response = match crate::limine::MEMORY_MAP_REQUEST.get_response() {
            Some(res) => res,
            None => {
                log::error!("BuddyFrameAllocator: Limine memory map response is missing");
                return 0;
            }
        };

        // 1. Determine highest usable physical address to size metadata slice
        let mut max_paddr: u64 = 0;
        for entry in memmap_response.entries() {
            if entry.entry_type == limine::memory_map::EntryType::USABLE {
                let end = entry.base + entry.length;
                if end > max_paddr {
                    max_paddr = end;
                }
            }
        }

        if max_paddr == 0 {
            log::error!("BuddyFrameAllocator: no usable physical RAM found");
            return 0;
        }

        let total_system_pages = ((max_paddr + PAGE_SIZE - 1) / PAGE_SIZE) as usize;
        let metadata_bytes = total_system_pages * core::mem::size_of::<PageFrameMetadata>();
        let metadata_aligned_bytes =
            (metadata_bytes + (PAGE_SIZE as usize) - 1) & !(PAGE_SIZE as usize - 1);

        // 2. Carve a usable memory region above 1 MiB for the metadata array
        let mut chosen_phys_base: u64 = 0;
        for entry in memmap_response.entries() {
            if entry.entry_type == limine::memory_map::EntryType::USABLE {
                let region_start = entry.base.max(0x100_000); // Protect sub-1MB legacy BIOS
                let aligned_start = (region_start + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
                let aligned_end = (entry.base + entry.length) & !(PAGE_SIZE - 1);

                if aligned_end > aligned_start
                    && (aligned_end - aligned_start) >= metadata_aligned_bytes as u64
                {
                    chosen_phys_base = aligned_start;
                    break;
                }
            }
        }

        if chosen_phys_base == 0 {
            log::error!(
                "BuddyFrameAllocator: failed to locate usable region for metadata (needed {} bytes)",
                metadata_aligned_bytes
            );
            return 0;
        }

        let metadata_phys_end = chosen_phys_base + metadata_aligned_bytes as u64;
        let metadata_virt_ptr = (chosen_phys_base + hhdm_offset) as *mut PageFrameMetadata;

        // 3. Zero and initialize metadata slice
        // SAFETY: chosen_phys_base is an exclusive, contiguous usable range in RAM mapped via HHDM.
        unsafe {
            core::ptr::write_bytes(metadata_virt_ptr as *mut u8, 0, metadata_aligned_bytes);
            let slice = core::slice::from_raw_parts_mut(metadata_virt_ptr, total_system_pages);
            for item in slice.iter_mut() {
                *item = PageFrameMetadata::new();
            }
            self.metadata = Some(slice);
        }

        // Mark the metadata's own physical frame range as RESERVED
        let meta_start_idx = (chosen_phys_base >> PAGE_SHIFT) as usize;
        let meta_end_idx = (metadata_phys_end >> PAGE_SHIFT) as usize;
        if let Some(ref mut meta) = self.metadata {
            for i in meta_start_idx..meta_end_idx.min(meta.len()) {
                meta[i].flags = PageFrameFlags::RESERVED;
            }
        }

        // 4. Populate free blocks from usable memory map entries
        for entry in memmap_response.entries() {
            if entry.entry_type != limine::memory_map::EntryType::USABLE {
                continue;
            }

            let region_start = entry.base.max(0x100_000);
            let aligned_start = (region_start + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
            let aligned_end = (entry.base + entry.length) & !(PAGE_SIZE - 1);

            if aligned_end <= aligned_start {
                continue;
            }

            // Exclude the metadata array range
            if aligned_start < metadata_phys_end && aligned_end > chosen_phys_base {
                if aligned_start < chosen_phys_base {
                    self.add_usable_range(aligned_start, chosen_phys_base);
                }
                if aligned_end > metadata_phys_end {
                    self.add_usable_range(metadata_phys_end, aligned_end);
                }
            } else {
                self.add_usable_range(aligned_start, aligned_end);
            }
        }

        self.total_pages
    }

    /// Decomposes an arbitrary page-aligned range `[start_phys, end_phys)` into maximal
    /// naturally aligned power-of-two buddy blocks and enqueues them into free lists.
    fn add_usable_range(&mut self, start_phys: u64, end_phys: u64) {
        let meta = match self.metadata.as_deref_mut() {
            Some(m) => m,
            None => return,
        };

        let mut curr = start_phys;
        while curr < end_phys {
            let remaining_pages = ((end_phys - curr) >> PAGE_SHIFT) as usize;
            let page_idx = (curr >> PAGE_SHIFT) as usize;

            // Determine largest order naturally aligned and fitting within remaining range
            let mut order = 0;
            while order + 1 < FRAME_BUDDY_ORDERS
                && (1usize << (order + 1)) <= remaining_pages
                && (page_idx & ((1usize << (order + 1)) - 1)) == 0
            {
                order += 1;
            }

            let count = 1usize << order;
            if page_idx + count <= meta.len() {
                meta[page_idx].flags = PageFrameFlags::USABLE;
                meta[page_idx].order = order as u8;
                for i in 1..count {
                    meta[page_idx + i].flags = PageFrameFlags::USABLE | PageFrameFlags::TAIL;
                    meta[page_idx + i].order = 255;
                }
                self.free_lists.push(order, page_idx, meta);
                self.total_pages += count;
            }
            curr += (count as u64) << PAGE_SHIFT;
        }
    }

    /// Allocate `2^order` contiguous, size-aligned physical pages.
    pub fn alloc_pages(&mut self, order: usize) -> Option<PhysAddr> {
        if order >= FRAME_BUDDY_ORDERS {
            return None;
        }
        let meta = self.metadata.as_deref_mut()?;

        // 1. Locate smallest available order k >= order
        let mut k = order;
        while k < FRAME_BUDDY_ORDERS && self.free_lists.is_empty(k) {
            k += 1;
        }
        if k == FRAME_BUDDY_ORDERS {
            return None; // Out of memory
        }

        // 2. Pop head block from order k
        let head_idx = self.free_lists.pop(k, meta)?;

        // 3. Repeatedly split down to requested order
        while k > order {
            k -= 1;
            let buddy_idx = head_idx + (1usize << k);

            if buddy_idx < meta.len() {
                meta[buddy_idx].order = k as u8;
                meta[buddy_idx].flags.remove(PageFrameFlags::TAIL);
                self.free_lists.push(k, buddy_idx, meta);
            }
        }

        // 4. Mark block as allocated
        let count = 1usize << order;
        if head_idx + count > meta.len() {
            return None;
        }
        meta[head_idx].set_allocated(order as u8);
        for i in 1..count {
            meta[head_idx + i]
                .flags
                .insert(PageFrameFlags::ALLOCATED | PageFrameFlags::TAIL);
            meta[head_idx + i].order = 255;
        }
        self.allocated_pages += count;

        Some(page_idx_to_phys(head_idx))
    }

    /// Allocate a single 4 KiB physical page (order 0).
    #[inline(always)]
    pub fn alloc_page(&mut self) -> Option<PhysAddr> {
        self.alloc_pages(0)
    }

    /// Free a block of `2^order` physical pages, coalescing with buddy blocks.
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
        let meta = match self.metadata.as_deref_mut() {
            Some(m) => m,
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

        if meta[head_idx].is_tail() {
            log::error!(
                "free_pages: attempting to free non-head buddy page at {:#x}",
                paddr.as_u64()
            );
            return;
        }

        let alloc_order = meta[head_idx].order as usize;
        let actual_order = if order != alloc_order {
            log::warn!(
                "free_pages: order mismatch at {:#x}: requested {}, recorded {}. Using recorded order.",
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
            // Frame is still referenced by another shared/COW mapping
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

        // Coalesce with buddy blocks
        let mut curr_idx = head_idx;
        let mut curr_order = actual_order;

        while curr_order + 1 < FRAME_BUDDY_ORDERS {
            let buddy_idx = curr_idx ^ (1usize << curr_order);
            let buddy_count = 1usize << curr_order;

            if buddy_idx + buddy_count > meta.len() {
                break;
            }
            if !meta[buddy_idx].is_usable() {
                break;
            }
            if meta[buddy_idx].is_allocated() {
                break;
            }
            if meta[buddy_idx].is_reserved() {
                break;
            }
            if meta[buddy_idx].is_tail() {
                break;
            }
            if meta[buddy_idx].order != curr_order as u8 {
                break;
            }

            // Buddy is free and at the exact same order: unlink from free list
            self.free_lists.remove(curr_order, buddy_idx, meta);

            // Merge into block of curr_order + 1
            let merged_idx = curr_idx.min(buddy_idx);
            let other_idx = curr_idx.max(buddy_idx);

            meta[other_idx].flags.insert(PageFrameFlags::TAIL);
            meta[other_idx].order = 255;

            curr_idx = merged_idx;
            curr_order += 1;
            meta[curr_idx].order = curr_order as u8;
        }

        // Insert coalesced block into free list
        meta[curr_idx].order = curr_order as u8;
        meta[curr_idx].flags.remove(PageFrameFlags::TAIL);
        self.free_lists.push(curr_order, curr_idx, meta);
    }

    /// Free a single 4 KiB physical page (order 0).
    #[inline(always)]
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
    #[inline(always)]
    pub fn total_pages(&self) -> usize {
        self.total_pages
    }

    /// Returns the number of currently free pages in the system.
    #[inline(always)]
    pub fn free_pages_count(&self) -> usize {
        self.total_pages.saturating_sub(self.allocated_pages)
    }
}
