//! Virtual Memory Protection Management (`mprotect`) for PetraOS.
//!
//! Provides region-based memory protection updates, splitting/merging VMAs,
//! and synchronizing hardware page table permissions (with Copy-On-Write awareness).

use crate::mm::vmm::paging::PageTable;
use crate::mm::vmm::types::VmAreaKind;
use crate::mm::vmm::vma::{AddrSpace, AddrSpaceError, COW_FLAG, VmArea};
use alloc::sync::Arc;
use alloc::vec::Vec;
use x86_64::structures::paging::PageTableFlags;
use x86_64::VirtAddr;

impl<P: PageTable> AddrSpace<P> {
    /// Modify protection flags for the virtual memory range `[start, start + size)`.
    ///
    /// # Semantics
    /// 1. `start` must be aligned to 4096 bytes.
    /// 2. If `size == 0`, returns `Ok(())` immediately.
    /// 3. The entire range must be continuously backed by existing VMAs with no unmapped holes.
    /// 4. Boundary VMAs overlapping `start` or `end` are split.
    /// 5. Overlapping VMAs are updated with `new_flags`.
    /// 6. Page table entries for pages currently mapped in hardware are remapped (preserving COW if shared).
    /// 7. Adjacent VMAs with identical permissions and compatible kinds are merged.
    pub fn mprotect_range(
        &mut self,
        start: VirtAddr,
        size: usize,
        new_flags: PageTableFlags,
    ) -> Result<(), AddrSpaceError> {
        if size == 0 {
            return Ok(());
        }

        if !start.is_aligned(4096u64) {
            return Err(AddrSpaceError::InvalidRange);
        }

        let aligned_size = (size + 4095) & !4095;
        let end = match start.as_u64().checked_add(aligned_size as u64) {
            Some(e) => VirtAddr::new(e),
            None => return Err(AddrSpaceError::InvalidRange),
        };

        // 1. Verify that the entire range [start, end) is completely covered by existing VMAs.
        let mut curr = start;
        while curr < end {
            if let Some(vma) = self.find_vma(curr) {
                if vma.end <= curr {
                    return Err(AddrSpaceError::UnmappedRange);
                }
                curr = vma.end;
            } else {
                return Err(AddrSpaceError::UnmappedRange);
            }
        }

        // 2. Split VMAs crossing boundary addresses 'start' and 'end'.
        self.split_vma_at(start);
        self.split_vma_at(end);

        // 3. Update VMA flags for all VMAs within [start, end).
        for (_, vma) in self.vm_areas.range_mut(start..end) {
            vma.flags = new_flags;
        }

        // 4. Update hardware page table entries for mapped pages in [start, end).
        for page_virt_u64 in (start.as_u64()..end.as_u64()).step_by(4096) {
            let page_virt = VirtAddr::new(page_virt_u64);
            if let Some((phys_frame, entry_flags)) = self.page_table.get_entry(page_virt) {
                let is_cow = entry_flags.contains(COW_FLAG);
                let ref_count = crate::mm::PMM.get_ref(phys_frame);

                let pte_flags = if new_flags.contains(PageTableFlags::WRITABLE) && (is_cow || ref_count > 1) {
                    // Page is shared under COW: keep PTE read-only with COW_FLAG
                    (new_flags & !PageTableFlags::WRITABLE) | COW_FLAG
                } else {
                    new_flags & !COW_FLAG
                };

                let _ = self.page_table.remap(page_virt, pte_flags);
            }
        }

        // 5. Merge contiguous adjacent VMAs that have identical flags and compatible kinds.
        self.merge_adjacent_vmas();

        Ok(())
    }

    /// Split any VMA that contains `addr` strictly inside its boundaries (`vma.start < addr < vma.end`).
    pub fn split_vma_at(&mut self, addr: VirtAddr) {
        if !addr.is_aligned(4096u64) {
            return;
        }

        let target_vma_start = self
            .vm_areas
            .range(..addr)
            .next_back()
            .map(|(&start, vma)| (start, vma.clone()))
            .filter(|(_, vma)| addr > vma.start && addr < vma.end);

        if let Some((start_key, old_vma)) = target_vma_start {
            self.vm_areas.remove(&start_key);

            let split_offset = (addr - old_vma.start) as usize;

            let (left_kind, right_kind) = match &old_vma.kind {
                VmAreaKind::Anonymous => (VmAreaKind::Anonymous, VmAreaKind::Anonymous),
                VmAreaKind::Device { phys_start } => (
                    VmAreaKind::Device {
                        phys_start: *phys_start,
                    },
                    VmAreaKind::Device {
                        phys_start: *phys_start + split_offset as u64,
                    },
                ),
                VmAreaKind::File {
                    file,
                    offset,
                    file_size,
                } => (
                    VmAreaKind::File {
                        file: Arc::clone(file),
                        offset: *offset,
                        file_size: *file_size,
                    },
                    VmAreaKind::File {
                        file: Arc::clone(file),
                        offset: *offset + split_offset,
                        file_size: *file_size,
                    },
                ),
            };

            let left_vma = VmArea {
                start: old_vma.start,
                end: addr,
                flags: old_vma.flags,
                kind: left_kind,
            };

            let right_vma = VmArea {
                start: addr,
                end: old_vma.end,
                flags: old_vma.flags,
                kind: right_kind,
            };

            self.vm_areas.insert(left_vma.start, left_vma);
            self.vm_areas.insert(right_vma.start, right_vma);
        }
    }

    /// Check if two VMAs can be merged into a single continuous VMA.
    fn can_merge_vmas(cur: &VmArea, next: &VmArea) -> bool {
        if cur.flags != next.flags {
            return false;
        }
        match (&cur.kind, &next.kind) {
            (VmAreaKind::Anonymous, VmAreaKind::Anonymous) => true,
            (VmAreaKind::Device { phys_start: p1 }, VmAreaKind::Device { phys_start: p2 }) => {
                p2.as_u64() == p1.as_u64() + (cur.end - cur.start)
            }
            (
                VmAreaKind::File {
                    file: f1,
                    offset: o1,
                    file_size: s1,
                },
                VmAreaKind::File {
                    file: f2,
                    offset: o2,
                    file_size: s2,
                },
            ) => Arc::ptr_eq(f1, f2) && s1 == s2 && *o2 == *o1 + (cur.end - cur.start) as usize,
            _ => false,
        }
    }

    /// Merge adjacent VMAs that share identical protection and compatible backing descriptors.
    pub fn merge_adjacent_vmas(&mut self) {
        let keys: Vec<VirtAddr> = self.vm_areas.keys().copied().collect();
        let mut i = 0;
        while i + 1 < keys.len() {
            let current_key = keys[i];
            let next_key = keys[i + 1];

            let can_merge = match (self.vm_areas.get(&current_key), self.vm_areas.get(&next_key)) {
                (Some(cur), Some(next)) => cur.end == next.start && Self::can_merge_vmas(cur, next),
                _ => false,
            };

            if can_merge {
                if let Some(next_vma) = self.vm_areas.remove(&next_key) {
                    if let Some(cur_vma) = self.vm_areas.get_mut(&current_key) {
                        cur_vma.end = next_vma.end;
                    }
                }
                // Continue merging iteratively
                return self.merge_adjacent_vmas();
            }
            i += 1;
        }
    }
}
