//! Physical Frame Allocator (Buddy System).
//!
//! Manages physical page frames (4 KiB units) using the `buddy_system_allocator` crate,
//! coupled with page-level metadata tracking for reference counting (COW) and safe double-free guards.

use super::frame::{phys_to_page_idx, PageFrameFlags, PageFrameMetadata, PAGE_SIZE};
use crate::mm::PhysAddr;
use buddy_system_allocator::Heap;
use core::alloc::Layout;
use core::ptr::NonNull;

/// Maximum order supported by the buddy heap (order 0..32, up to 4 GiB blocks).
pub const BUDDY_MAX_ORDER: usize = 32;

/// Physical memory manager utilizing buddy system allocation.
pub struct BuddyFrameAllocator {
    heap: Heap<BUDDY_MAX_ORDER>,
    metadata: Option<&'static mut [PageFrameMetadata]>,
    hhdm_offset: u64,
    total_pages: usize,
    allocated_pages: usize,
}

unsafe impl Send for BuddyFrameAllocator {}
unsafe impl Sync for BuddyFrameAllocator {}

impl BuddyFrameAllocator {
    /// Creates an uninitialized buddy frame allocator instance.
    pub const fn new() -> Self {
        Self {
            heap: Heap::new(),
            metadata: None,
            hhdm_offset: 0,
            total_pages: 0,
            allocated_pages: 0,
        }
    }

    /// Initializes the buddy allocator with available memory map entries
    /// and pre-allocated page metadata slice.
    pub fn init(
        &mut self,
        metadata: &'static mut [PageFrameMetadata],
        metadata_phys_start: u64,
        metadata_phys_end: u64,
        hhdm_offset: u64,
    ) {
        self.metadata = Some(metadata);
        self.hhdm_offset = hhdm_offset;

        let memmap_response = crate::limine::MEMORY_MAP_REQUEST
            .get_response()
            .expect("BuddyAllocator init: Limine memory map response is missing");

        let mut total_usable_pages = 0;

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
                    self.add_usable_range(aligned_start, metadata_phys_start, &mut total_usable_pages);
                }
                // Segment 2: after metadata
                if aligned_end > metadata_phys_end {
                    self.add_usable_range(metadata_phys_end, aligned_end, &mut total_usable_pages);
                }
            } else {
                self.add_usable_range(aligned_start, aligned_end, &mut total_usable_pages);
            }
        }

        self.total_pages = total_usable_pages;
        log::info!(
            "BuddyFrameAllocator: initialized with {} usable pages (~{} MiB)",
            total_usable_pages,
            (total_usable_pages * (PAGE_SIZE as usize)) / (1024 * 1024)
        );
    }

    /// Helper to register and populate a usable physical memory range into the buddy heap.
    fn add_usable_range(&mut self, start_phys: u64, end_phys: u64, total_pages: &mut usize) {
        let meta = match self.metadata {
            Some(ref mut m) => m,
            None => return,
        };

        let start_idx = phys_to_page_idx(PhysAddr::new(start_phys));
        let end_idx = phys_to_page_idx(PhysAddr::new(end_phys)).min(meta.len());

        for i in start_idx..end_idx {
            meta[i].flags.insert(PageFrameFlags::USABLE);
            *total_pages += 1;
        }

        let start_virt = (start_phys + self.hhdm_offset) as usize;
        let end_virt = (end_phys + self.hhdm_offset) as usize;

        // SAFETY: The physical range is verified as usable RAM and accessed via valid HHDM.
        unsafe {
            self.heap.add_to_heap(start_virt, end_virt);
        }
    }

    /// Allocate $2^{\text{order}}$ physical memory pages.
    pub fn alloc_pages(&mut self, order: usize) -> Option<PhysAddr> {
        if order >= BUDDY_MAX_ORDER {
            return None;
        }

        let size = (1usize << order) * (PAGE_SIZE as usize);
        let layout = match Layout::from_size_align(size, size) {
            Ok(l) => l,
            Err(_) => return None,
        };

        let vptr = match self.heap.alloc(layout) {
            Ok(p) => p,
            Err(_) => return None,
        };

        let paddr_val = (vptr.as_ptr() as u64) - self.hhdm_offset;
        let head_idx = phys_to_page_idx(PhysAddr::new(paddr_val));

        if let Some(ref mut meta) = self.metadata {
            if head_idx < meta.len() {
                meta[head_idx].set_allocated(order as u8);
                let count = 1usize << order;
                for i in 1..count {
                    if head_idx + i < meta.len() {
                        meta[head_idx + i].flags.insert(PageFrameFlags::ALLOCATED);
                    }
                }
            }
        }

        self.allocated_pages += 1usize << order;
        Some(PhysAddr::new(paddr_val))
    }

    /// Allocate a single 4 KiB physical page (order 0).
    pub fn alloc_page(&mut self) -> Option<PhysAddr> {
        self.alloc_pages(0)
    }

    /// Free a block of $2^{\text{order}}$ physical pages back to the buddy allocator.
    pub fn free_pages(&mut self, paddr: PhysAddr, order: usize) {
        if !paddr.is_aligned(PAGE_SIZE) {
            log::error!("free_pages: address {:#x} is not page-aligned", paddr.as_u64());
            return;
        }

        if paddr.as_u64() < 0x1000 {
            log::warn!("free_pages: refusing to free sub-page-0 address {:#x}", paddr.as_u64());
            return;
        }

        let head_idx = phys_to_page_idx(paddr);
        let meta = match self.metadata {
            Some(ref mut m) => m,
            None => return,
        };

        if head_idx >= meta.len() {
            log::error!("free_pages: page index {} out of bounds for {:#x}", head_idx, paddr.as_u64());
            return;
        }

        if !meta[head_idx].is_usable() {
            log::warn!("free_pages: attempting to free non-usable page at {:#x}", paddr.as_u64());
            return;
        }

        if !meta[head_idx].is_allocated() {
            log::warn!("free_pages: double free detected at {:#x}", paddr.as_u64());
            return;
        }

        let new_ref = meta[head_idx].dec_ref();
        if new_ref > 0 {
            // Page is still referenced by another shared mapping (e.g. COW)
            return;
        }

        let count = 1usize << order;
        for i in 0..count {
            if head_idx + i < meta.len() {
                meta[head_idx + i].clear_allocated();
            }
        }
        self.allocated_pages = self.allocated_pages.saturating_sub(count);

        let size = (1usize << order) * (PAGE_SIZE as usize);
        if let Ok(layout) = Layout::from_size_align(size, size) {
            let vptr_val = (paddr.as_u64() + self.hhdm_offset) as *mut u8;
            if let Some(nonnull) = NonNull::new(vptr_val) {
                // SAFETY: We verified the page was allocated, ref_count reached 0, and bounds are valid.
                unsafe {
                    self.heap.dealloc(nonnull, layout);
                }
            }
        }
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
