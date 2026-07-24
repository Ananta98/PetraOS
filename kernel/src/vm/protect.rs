//! Memory protection (`mprotect`) for virtual memory areas.
//!
//! This module implements the [`VmaManager::mprotect`] and [`VmaManager::protect_pages`]
//! methods, which apply new [`PageFlags`] to a range of virtual addresses.
//!
//! # Split Strategy
//!
//! Because a single `mprotect` call may touch only *part* of an existing VMA,
//! the implementation may need to split one region into up to three pieces:
//!
//! ```text
//!   Before:  [──────────── VMA ────────────]
//!   Range:         [──── mprotect ────]
//!   After:   [─ A ─][──── B (new) ────][─ C ─]
//! ```
//!
//! The four overlap cases handled are:
//!
//! | Case | Condition | Result |
//! |------|-----------|--------|
//! | Full cover  | range ⊇ VMA | VMA flags replaced in-place |
//! | Split middle | range ⊂ VMA | VMA split into left, new, right |
//! | Right trim  | range overlaps VMA right | VMA split into left, new |
//! | Left trim   | range overlaps VMA left  | VMA split into new, right |

use crate::vm::{region::VmaRegion, vma::VmaManager};
use alloc::vec::Vec;
use ostd::{
    Error,
    mm::{CachePolicy, PAGE_SIZE, PageFlags, PageProperty, Vaddr, vm_space::VmQueriedItem},
    task::disable_preempt,
};

impl VmaManager {
    /// Updates the hardware page-table protection flags for a contiguous range of pages.
    ///
    /// For each page in `start..start + size`:
    /// - If the page is backed by a RAM frame, it is **unmapped and remapped** with
    ///   the new flags (the physical frame is preserved).
    /// - If the page is not mapped, it is unmapped (no-op in practice).
    ///
    /// This function operates directly on the page tables; it does **not** update
    /// the [`VmaRegion`] metadata. Callers that need consistent metadata should use
    /// [`VmaManager::mprotect`] instead.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidArgs`] if a cursor cannot be created or navigated.
    pub fn protect_pages(
        &self,
        start: Vaddr,
        size: usize,
        new_flags: PageFlags,
    ) -> Result<(), Error> {
        let guard = disable_preempt();
        let end = start.checked_add(size).ok_or(Error::InvalidArgs)?;
        let vaddr_range = start..end;

        let mut cursor = self
            .vm_space
            .cursor_mut(&guard, &vaddr_range)
            .map_err(|_| Error::InvalidArgs)?;

        let num_pages = size / PAGE_SIZE;

        for page_index in 0..num_pages {
            let page_vaddr = start + (page_index * PAGE_SIZE);
            cursor.jump(page_vaddr).map_err(|_| Error::InvalidArgs)?;

            let (_, queried_item) = cursor.query().map_err(|_| Error::InvalidArgs)?;

            if let Some(VmQueriedItem::MappedRam { frame, prop: _ }) = queried_item {
                // Preserve the physical frame but apply the new protection flags.
                let frame_ref = (*frame).clone();
                let new_property = PageProperty::new_user(new_flags, CachePolicy::Writeback);

                cursor.unmap(PAGE_SIZE);
                cursor.jump(page_vaddr).map_err(|_| Error::InvalidArgs)?;
                cursor.map(frame_ref, new_property);
            } else {
                // Ensure any stale page-table entry is cleared.
                cursor.unmap(PAGE_SIZE);
            }
        }

        Ok(())
    }

    /// Changes the protection flags of the virtual address range `start..start + size`.
    ///
    /// This is the high-level entry point that mirrors the POSIX `mprotect(2)` semantics:
    ///
    /// 1. **Validates alignment** — both `start` and `size` must be page-aligned.
    /// 2. **Updates [`VmaRegion`] metadata** — the region map is split or modified to
    ///    reflect the new flags.
    /// 3. **Updates page-table entries** — delegates to [`VmaManager::protect_pages`].
    ///
    /// # Errors
    ///
    /// - [`Error::InvalidArgs`] if `start` or `size` is not page-aligned.
    /// - [`Error::NoMemory`] if no VMA overlaps the requested range.
    /// - Any error propagated from [`VmaManager::protect_pages`].
    pub fn mprotect(&self, start: Vaddr, size: usize, new_flags: PageFlags) -> Result<(), Error> {
        if start % PAGE_SIZE != 0 || size % PAGE_SIZE != 0 {
            return Err(Error::InvalidArgs);
        }

        let range_start = start;
        let range_end = start.checked_add(size).ok_or(Error::InvalidArgs)?;

        let mut regions = self.regions.lock();

        // Collect all VMAs that overlap the target range before mutating the map.
        let overlapping_vmas = collect_overlapping_vmas(&regions, range_start, range_end);

        if overlapping_vmas.is_empty() {
            return Err(Error::NoMemory);
        }

        for vma in overlapping_vmas {
            regions.remove(&vma.start);
            apply_protection_split(&mut regions, &vma, range_start, range_end, new_flags);
        }

        // Release the regions lock before updating page-table entries to avoid
        // holding it across the (potentially slow) cursor operations.
        drop(regions);

        self.protect_pages(start, size, new_flags)
    }
}

/// Returns all [`VmaRegion`]s whose address range overlaps `[range_start, range_end)`.
fn collect_overlapping_vmas(
    regions: &alloc::collections::BTreeMap<Vaddr, VmaRegion>,
    range_start: Vaddr,
    range_end: Vaddr,
) -> Vec<VmaRegion> {
    regions
        .values()
        .filter(|region| region.end() > range_start && region.start < range_end)
        .cloned()
        .collect()
}

/// Inserts replacement [`VmaRegion`] entries after `mprotect` modifies an overlapping VMA.
fn apply_protection_split(
    regions: &mut alloc::collections::BTreeMap<Vaddr, VmaRegion>,
    vma: &VmaRegion,
    range_start: Vaddr,
    range_end: Vaddr,
    new_flags: PageFlags,
) {
    let mut current = vma.clone();

    if range_start > current.start {
        let (left, right) = current.split_at(range_start);
        regions.insert(left.start, left);
        current = right;
    }

    if range_end < current.end() {
        let (left, right) = current.split_at(range_end);
        regions.insert(right.start, right);
        current = left;
    }

    current.flags = new_flags;
    regions.insert(current.start, current);
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use crate::vm::VMA_MANAGER;
    use alloc::sync::Arc;
    use ostd::prelude::ktest;

    /// An exact-match `mprotect` (range == VMA) should update the single region in-place
    /// without splitting.
    #[ktest]
    fn test_mprotect_perfect_match() {
        crate::vm::init();
        let vma_manager = VMA_MANAGER.get().unwrap().clone();
        vma_manager.activate();

        vma_manager
            .map_region(0x60000, PAGE_SIZE, PageFlags::RW)
            .unwrap();

        vma_manager
            .mprotect(0x60000, PAGE_SIZE, PageFlags::R)
            .unwrap();

        let regions = vma_manager.regions.lock();
        let region = regions.get(&0x60000).unwrap();
        assert_eq!(region.flags, PageFlags::R, "flags must be updated to R");
        assert_eq!(region.size, PAGE_SIZE, "region size must be unchanged");
        drop(regions);

        vma_manager.unmap_region(0x60000, PAGE_SIZE).unwrap();
    }

    /// A `mprotect` targeting only the middle page of a 3-page VMA should produce
    /// three distinct regions with correct flags.
    #[ktest]
    fn test_mprotect_split_middle() {
        crate::vm::init();
        let vma_manager = VMA_MANAGER.get().unwrap().clone();
        vma_manager.activate();

        vma_manager
            .map_region(0x70000, PAGE_SIZE * 3, PageFlags::RW)
            .unwrap();

        vma_manager
            .mprotect(0x71000, PAGE_SIZE, PageFlags::R)
            .unwrap();

        let regions = vma_manager.regions.lock();
        assert_eq!(regions.len(), 3, "VMA must be split into 3 regions");

        let left = regions.get(&0x70000).unwrap();
        assert_eq!(left.flags, PageFlags::RW, "left region must keep RW");
        assert_eq!(left.size, PAGE_SIZE);

        let middle = regions.get(&0x71000).unwrap();
        assert_eq!(middle.flags, PageFlags::R, "middle region must be R");
        assert_eq!(middle.size, PAGE_SIZE);

        let right = regions.get(&0x72000).unwrap();
        assert_eq!(right.flags, PageFlags::RW, "right region must keep RW");
        assert_eq!(right.size, PAGE_SIZE);

        drop(regions);
        vma_manager.unmap_region(0x70000, PAGE_SIZE).unwrap();
        vma_manager.unmap_region(0x71000, PAGE_SIZE).unwrap();
        vma_manager.unmap_region(0x72000, PAGE_SIZE).unwrap();
    }

    /// A `mprotect` targeting only the first page of a 2-page VMA should split it
    /// into a protected first page and an unchanged second page.
    #[ktest]
    fn test_mprotect_split_left_and_right() {
        crate::vm::init();
        let vma_manager = VMA_MANAGER.get().unwrap().clone();
        vma_manager.activate();

        vma_manager
            .map_region(0x80000, PAGE_SIZE * 2, PageFlags::RW)
            .unwrap();

        vma_manager
            .mprotect(0x80000, PAGE_SIZE, PageFlags::R)
            .unwrap();

        let regions = vma_manager.regions.lock();
        assert_eq!(regions.len(), 2, "VMA must be split into 2 regions");
        assert_eq!(
            regions.get(&0x80000).unwrap().flags,
            PageFlags::R,
            "first page must be R"
        );
        assert_eq!(
            regions.get(&0x81000).unwrap().flags,
            PageFlags::RW,
            "second page must remain RW"
        );
        drop(regions);

        vma_manager.unmap_region(0x80000, PAGE_SIZE).unwrap();
        vma_manager.unmap_region(0x81000, PAGE_SIZE).unwrap();
    }
}
