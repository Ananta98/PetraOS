//! Virtual Memory Advice (`madvise`) for PetraOS.
//!
//! Provides region-based memory usage hints and physical page reclamation.
//! Supports standard Linux POSIX advices including `MADV_DONTNEED`, `MADV_WILLNEED`,
//! `MADV_FREE`, and advice hints used by userspace memory allocators.

use crate::mm::vmm::paging::{PageTable, VirtAddr};
use crate::mm::vmm::types::VmAreaKind;
use crate::mm::vmm::vma::{AddrSpace, AddrSpaceError};

/// Standard Linux `madvise` advice codes
pub const MADV_NORMAL: i32 = 0;
pub const MADV_RANDOM: i32 = 1;
pub const MADV_SEQUENTIAL: i32 = 2;
pub const MADV_WILLNEED: i32 = 3;
pub const MADV_DONTNEED: i32 = 4;
pub const MADV_FREE: i32 = 8;
pub const MADV_REMOVE: i32 = 9;
pub const MADV_DONTFORK: i32 = 10;
pub const MADV_DOFORK: i32 = 11;
pub const MADV_MERGEABLE: i32 = 12;
pub const MADV_UNMERGEABLE: i32 = 13;
pub const MADV_HUGEPAGE: i32 = 14;
pub const MADV_NOHUGEPAGE: i32 = 15;
pub const MADV_DONTDUMP: i32 = 16;
pub const MADV_DODUMP: i32 = 17;
pub const MADV_WIPEONFORK: i32 = 18;
pub const MADV_KEEPONFORK: i32 = 19;
pub const MADV_COLD: i32 = 20;
pub const MADV_PAGEOUT: i32 = 21;

impl<P: PageTable> AddrSpace<P> {
    /// Provide memory advice for the virtual address range `[start, start + size)`.
    ///
    /// # Semantics (Linux ABI)
    /// 1. `start` must be aligned to 4096 bytes.
    /// 2. If `size == 0`, returns `Ok(())` immediately.
    /// 3. The entire range must be continuously mapped by existing VMAs with no unmapped holes.
    /// 4. For `MADV_DONTNEED` and `MADV_FREE`:
    ///    - Unmaps anonymous physical pages in the range, returning them to the PMM.
    ///    - On subsequent access, the page fault handler allocates a fresh, zeroed page.
    /// 5. For other advices (`MADV_NORMAL`, `MADV_RANDOM`, `MADV_SEQUENTIAL`, `MADV_WILLNEED`, etc.),
    ///    validates the range and accepts the hint.
    pub fn madvise_range(
        &mut self,
        start: VirtAddr,
        size: usize,
        advice: i32,
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

        match advice {
            MADV_DONTNEED | MADV_FREE => {
                // Reclaim mapped anonymous pages in the range.
                // Any subsequent read/write will page fault and be allocated clean zeroed memory.
                for page_virt_u64 in (start.as_u64()..end.as_u64()).step_by(4096) {
                    let page_virt = VirtAddr::new(page_virt_u64);
                    if let Some(vma) = self.find_vma(page_virt) {
                        if matches!(vma.kind, VmAreaKind::Anonymous) {
                            if let Some((phys_frame, _)) = self.page_table.get_entry(page_virt) {
                                let _ = self.page_table.unmap(page_virt);
                                if crate::mm::PMM.dec_ref(phys_frame) == 0 {
                                    crate::mm::PMM.free_page(phys_frame);
                                }
                            }
                        }
                    }
                }
                Ok(())
            }
            MADV_NORMAL
            | MADV_RANDOM
            | MADV_SEQUENTIAL
            | MADV_WILLNEED
            | MADV_DONTFORK
            | MADV_DOFORK
            | MADV_MERGEABLE
            | MADV_UNMERGEABLE
            | MADV_HUGEPAGE
            | MADV_NOHUGEPAGE
            | MADV_DONTDUMP
            | MADV_DODUMP
            | MADV_WIPEONFORK
            | MADV_KEEPONFORK
            | MADV_COLD
            | MADV_PAGEOUT => {
                // Accepted valid Linux advice hints
                Ok(())
            }
            _ => Err(AddrSpaceError::InvalidRange),
        }
    }
}
