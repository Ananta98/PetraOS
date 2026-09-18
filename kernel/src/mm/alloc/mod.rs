//! Kernel Memory Allocation Subsystem.
//!
//! Provides a unified three-tier hybrid memory management architecture:
//! 1. Early Bootstrapping Bump Allocator (`bump`)
//! 2. Physical Page Frame Allocator (`buddy`): `FrameAllocator` index over
//!    frame numbers plus bump-allocated per-frame metadata
//! 3. Kernel SLUB Allocator (`slab`): size-class caches with a buddy-`Heap`
//!    fallback grown on demand from frames

pub mod buddy;
pub mod bump;
pub mod slab;

pub use buddy::{
    BuddyFrameAllocator, PageFrameFlags, PageFrameMetadata, FRAME_BUDDY_ORDERS, PAGE_SIZE,
    page_idx_to_phys, phys_to_page_idx,
};
pub use slab::{ALLOCATOR, SlabAllocator};

use crate::mm::PhysAddr;
use crate::sync::Mutex;

/// Global physical frame allocator instance protected by kernel mutex.
pub static FRAME_ALLOCATOR: Mutex<BuddyFrameAllocator> = Mutex::new(BuddyFrameAllocator::new());

/// High-level accessor providing drop-in compatibility with existing kernel subsystems.
pub struct PhysicalMemoryManager;

impl PhysicalMemoryManager {
    #[inline(always)]
    pub fn alloc_page(&self) -> Option<PhysAddr> {
        FRAME_ALLOCATOR.lock().alloc_page()
    }

    #[inline(always)]
    pub fn free_page(&self, paddr: PhysAddr) {
        FRAME_ALLOCATOR.lock().free_page(paddr);
    }

    #[inline(always)]
    pub fn alloc_pages(&self, order: usize) -> Option<PhysAddr> {
        FRAME_ALLOCATOR.lock().alloc_pages(order)
    }

    #[inline(always)]
    pub fn free_pages(&self, paddr: PhysAddr, order: usize) {
        FRAME_ALLOCATOR.lock().free_pages(paddr, order);
    }

    #[inline(always)]
    pub fn inc_ref(&self, paddr: PhysAddr) {
        FRAME_ALLOCATOR.lock().inc_ref(paddr);
    }

    #[inline(always)]
    pub fn dec_ref(&self, paddr: PhysAddr) -> u32 {
        FRAME_ALLOCATOR.lock().dec_ref(paddr)
    }

    #[inline(always)]
    pub fn get_ref(&self, paddr: PhysAddr) -> u32 {
        FRAME_ALLOCATOR.lock().get_ref(paddr)
    }

    #[inline(always)]
    pub fn total_pages(&self) -> usize {
        FRAME_ALLOCATOR.lock().total_pages()
    }

    #[inline(always)]
    pub fn free_pages_count(&self) -> usize {
        FRAME_ALLOCATOR.lock().free_pages_count()
    }
}

pub static PMM: PhysicalMemoryManager = PhysicalMemoryManager;

/// Initialize physical and virtual memory allocation subsystems.
///
/// Order matters: bump-carved metadata → static early heap (frame-independent)
/// → frame buddy (index traffic lands in the early heap) → fallback-heap
/// pre-seed (may pull frames). Logging happens after the frame lock is
/// released so boot logs never nest inside the frame critical section.
pub fn init() {
    let hhdm = crate::mm::hhdm_offset();
    let bump_res = bump::early_allocate_metadata(hhdm);
    slab::early_init();
    let total_pages = FRAME_ALLOCATOR.lock().init(
        bump_res.metadata,
        bump_res.metadata_phys_start,
        bump_res.metadata_phys_end,
    );
    log::info!(
        "BuddyFrameAllocator: initialized with {} usable pages (~{} MiB)",
        total_pages,
        (total_pages * (PAGE_SIZE as usize)) / (1024 * 1024)
    );
    slab::init();
}
