//! Physical Page Frame Metadata and State Tracking.
//!
//! Provides metadata tracking per 4 KiB physical page frame including
//! reference counting for Copy-On-Write (COW), allocation flags, and slab association.

use crate::mm::PhysAddr;
use bitflags::bitflags;

pub const PAGE_SIZE: u64 = 4096;
pub const PAGE_SHIFT: u64 = 12;

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
        /// Page is managed by the kernel SLUB cache.
        const SLAB      = 1 << 3;
    }
}

/// Metadata tracked for each 4 KiB physical frame in the system.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct PageFrameMetadata {
    pub ref_count: u32,
    pub flags: PageFrameFlags,
    pub order: u8,
    pub slab_class: u8,
}

impl PageFrameMetadata {
    pub const fn new() -> Self {
        Self {
            ref_count: 0,
            flags: PageFrameFlags::empty(),
            order: 0,
            slab_class: 0,
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
    pub fn is_slab(&self) -> bool {
        self.flags.contains(PageFrameFlags::SLAB)
    }

    #[inline(always)]
    pub fn set_allocated(&mut self, order: u8) {
        self.flags.insert(PageFrameFlags::ALLOCATED);
        self.order = order;
        self.ref_count = 1;
    }

    #[inline(always)]
    pub fn clear_allocated(&mut self) {
        self.flags.remove(PageFrameFlags::ALLOCATED | PageFrameFlags::SLAB);
        self.order = 0;
        self.slab_class = 0;
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
