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
            let vma_end = vma.start + vma.size;
            regions.remove(&vma.start);

            apply_protection_split(
                &mut regions,
                &vma,
                vma_end,
                range_start,
                range_end,
                new_flags,
                size,
            );
        }

        // Release the regions lock before updating page-table entries to avoid
        // holding it across the (potentially slow) cursor operations.
        drop(regions);

        self.protect_pages(start, size, new_flags)
    }
}

/// Returns all [`VmaRegion`]s whose address range overlaps `[range_start, range_end)`.
///
/// A region overlaps if it does not end at or before `range_start` and does not
/// start at or after `range_end`.
fn collect_overlapping_vmas(
    regions: &alloc::collections::BTreeMap<Vaddr, VmaRegion>,
    range_start: Vaddr,
    range_end: Vaddr,
) -> Vec<VmaRegion> {
    regions
        .values()
        .filter(|region| {
            let region_end = region.start.saturating_add(region.size);
            // Exclude regions that are entirely before or entirely after the range.
            region_end > range_start && region.start < range_end
        })
        .cloned()
        .collect()
}

/// Inserts replacement [`VmaRegion`] entries after a `mprotect` call has removed
/// an overlapping VMA.
///
/// Depending on how the target range intersects the existing VMA, the VMA is
/// replaced by one, two, or three new entries:
///
/// - **Full cover**: the target completely covers the VMA → single entry with `new_flags`.
/// - **Split middle**: the target is a strict sub-range of the VMA → three entries
///   (left tail, new middle, right tail).
/// - **Right trim**: the target overlaps only the right side → two entries
///   (unchanged left, new right).
/// - **Left trim**: the target overlaps only the left side → two entries
///   (new left, unchanged right).
fn apply_protection_split(
    regions: &mut alloc::collections::BTreeMap<Vaddr, VmaRegion>,
    vma: &VmaRegion,
    vma_end: Vaddr,
    range_start: Vaddr,
    range_end: Vaddr,
    new_flags: PageFlags,
    new_size: usize,
) {
    if range_start <= vma.start && range_end >= vma_end {
        // ── Case 1: Target fully covers the VMA ────────────────────────────────
        // Replace the VMA in-place with only its flags changed.
        regions.insert(
            vma.start,
            VmaRegion {
                flags: new_flags,
                ..vma.clone()
            },
        );
    } else if vma.start < range_start && vma_end > range_end {
        // ── Case 2: Target is a strict sub-range of the VMA (split into 3) ─────
        //
        //   [─── left (unchanged) ─────][─── middle (new_flags) ───][─── right (unchanged) ───]
        let left_size = range_start - vma.start;
        let right_offset = range_end - vma.start;

        regions.insert(
            vma.start,
            VmaRegion {
                size: left_size,
                ..vma.clone()
            },
        );
        regions.insert(
            range_start,
            VmaRegion {
                start: range_start,
                size: new_size,
                flags: new_flags,
                guard_size: 0,
                file_backing: vma.file_backing.clone(),
                file_offset: vma.file_offset + left_size,
                is_shared: vma.is_shared,
            },
        );
        regions.insert(
            range_end,
            VmaRegion {
                start: range_end,
                size: vma_end - range_end,
                flags: vma.flags,
                guard_size: 0,
                file_backing: vma.file_backing.clone(),
                file_offset: vma.file_offset + right_offset,
                is_shared: vma.is_shared,
            },
        );
    } else if vma.start < range_start && vma_end <= range_end {
        // ── Case 3: Target overlaps the right portion of the VMA ───────────────
        //
        //   [─── left (unchanged) ─────][─── right (new_flags) ───]
        let left_size = range_start - vma.start;
        let right_offset = range_start - vma.start;

        regions.insert(
            vma.start,
            VmaRegion {
                size: left_size,
                ..vma.clone()
            },
        );
        regions.insert(
            range_start,
            VmaRegion {
                start: range_start,
                size: vma_end - range_start,
                flags: new_flags,
                guard_size: 0,
                file_backing: vma.file_backing.clone(),
                file_offset: vma.file_offset + right_offset,
                is_shared: vma.is_shared,
            },
        );
    } else if vma.start >= range_start && vma_end > range_end {
        // ── Case 4: Target overlaps the left portion of the VMA ────────────────
        //
        //   [─── left (new_flags) ───][─── right (unchanged) ───]
        let right_offset = range_end - vma.start;

        regions.insert(
            vma.start,
            VmaRegion {
                size: range_end - vma.start,
                flags: new_flags,
                ..vma.clone()
            },
        );
        regions.insert(
            range_end,
            VmaRegion {
                start: range_end,
                size: vma_end - range_end,
                flags: vma.flags,
                guard_size: 0,
                file_backing: vma.file_backing.clone(),
                file_offset: vma.file_offset + right_offset,
                is_shared: vma.is_shared,
            },
        );
    }
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
