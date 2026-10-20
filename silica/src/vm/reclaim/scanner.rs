// SPDX-License-Identifier: GPL-2.0

//! An address space scanning its own memory to release clean pages.

use alloc::vec::Vec;
use core::ops::Range;

use ostd::mm::{PAGE_SIZE, PageFlags, Vaddr, VmSpace, vm_space::VmQueriedItem};
use ostd::task::{DisabledPreemptGuard, disable_preempt};

use crate::vm::reclaim::ReclaimSource;
use crate::vm::vmar::{Vmar, VmarInner};

impl Vmar {
    /// Returns the addresses of up to `budget` pages of this address space that
    /// reclaim may drop, in ascending address order.
    pub fn reclaim_pages(&self, budget: usize) -> usize {
        ReclaimSource::reclaim(self, budget)
    }

    /// Releases up to `budget` pages, taking the records lock once for the whole
    /// pass so that a concurrent `mmap` or `munmap` cannot move the mappings
    /// this pass is walking out from under it.
    pub(crate) fn reclaim_locked(&self, budget: usize) -> usize {
        let guard = disable_preempt();
        let records = self.inner.write();

        let candidates = collect_reclaimable(&records, &self.vm_space, budget, &guard);
        if candidates.is_empty() {
            return 0;
        }
        unmap_candidates(&self.vm_space, &candidates)
    }
}

/// Returns the addresses of up to `budget` pages that reclaim may drop, in
/// ascending address order.
fn collect_reclaimable(
    inner: &VmarInner,
    vm_space: &VmSpace,
    budget: usize,
    guard: &DisabledPreemptGuard,
) -> Vec<Vaddr> {
    let mut candidates: Vec<Vaddr> = Vec::new();

    for mapping in inner.mappings.iter() {
        if !mapping.is_reclaimable() {
            continue;
        }
        let range: Range<Vaddr> = mapping.range();
        let Ok(mut cursor) = vm_space.cursor(guard, &range) else {
            continue;
        };

        while candidates.len() < budget {
            let Some(remaining) = range.end.checked_sub(cursor.virt_addr()) else {
                break;
            };
            let Some(mapped) = cursor.find_next(remaining) else {
                break;
            };
            let (_, Some(item)) = cursor.query().expect("a mapped page was just found") else {
                break;
            };
            match item {
                VmQueriedItem::MappedRam { frame, prop } => {
                    if !prop.flags.contains(PageFlags::DIRTY) && frame.reference_count() == 1 {
                        candidates.push(mapped);
                    }
                }
                VmQueriedItem::MappedIoMem { .. } => break,
            }
            if cursor.jump(mapped + ostd::mm::PAGE_SIZE).is_err() {
                break;
            }
        }

        if candidates.len() >= budget {
            break;
        }
    }
    candidates
}

/// Unmaps `candidates`, which must be page-aligned and ascending, and returns
/// how many pages were released.
fn unmap_candidates(vm_space: &VmSpace, candidates: &[Vaddr]) -> usize {
    let mut freed = 0;
    let mut index = 0;

    while index < candidates.len() {
        let run_start = candidates[index];
        let mut run_end = run_start + PAGE_SIZE;
        while index + 1 < candidates.len() && candidates[index + 1] == run_end {
            index += 1;
            run_end += PAGE_SIZE;
        }
        index += 1;

        let guard = disable_preempt();
        let Ok(mut cursor) = vm_space.cursor_mut(&guard, &(run_start..run_end)) else {
            continue;
        };
        freed += cursor.unmap(run_end - run_start);
        cursor.flusher().sync_tlb_flush();
    }
    freed
}

impl ReclaimSource for Vmar {
    fn reclaim(&self, budget: usize) -> usize {
        if budget == 0 {
            return 0;
        }
        self.reclaim_locked(budget)
    }
}

#[cfg(ktest)]
mod tests {
    use alloc::sync::Arc;
    use ostd::mm::{PAGE_SIZE, Vaddr};
    use ostd::prelude::ktest;

    use super::*;
    use crate::vm::backing::Backing;
    use crate::vm::flags::MmapFlags;
    use crate::vm::perms::VmPerms;
    use crate::vm::shm::SharedPages;
    use crate::vm::tests::{not_resident, record_at};
    use crate::vm::vmar::page_fault::PageFaultInfo;

    const BASE: Vaddr = 0x4000_0000;

    fn anon(vmar: &Vmar, pages: usize) -> Vaddr {
        vmar.mmap_anonymous(
            BASE,
            pages * PAGE_SIZE,
            VmPerms::READ | VmPerms::WRITE,
            MmapFlags::PRIVATE,
        )
        .expect("the mapping is created at the requested address")
    }

    fn fault_in_cleanly(vmar: &Vmar, start: Vaddr, pages: usize) {
        for page in 0..pages {
            vmar.handle_page_fault(&PageFaultInfo::new(start + page * PAGE_SIZE, VmPerms::READ))
                .expect("the page faults in");
        }
    }

    #[ktest]
    fn a_clean_untouched_page_is_given_back() {
        let vmar = Vmar::new();
        let start = anon(&vmar, 2);
        fault_in_cleanly(&vmar, start, 1);
        assert!(!not_resident(&vmar, start), "the page is resident");

        assert_eq!(vmar.reclaim_pages(8), 1, "the clean page is released");
        assert!(not_resident(&vmar, start), "its page table entry is gone");
        assert_eq!(record_at(&vmar, start).len(), 2 * PAGE_SIZE);
    }

    #[ktest]
    fn a_written_page_is_kept() {
        let vmar = Vmar::new();
        let start = anon(&vmar, 1);

        vmar.handle_page_fault(&PageFaultInfo::new(start, VmPerms::WRITE))
            .expect("the page faults in");
        assert!(!not_resident(&vmar, start));

        assert_eq!(
            vmar.reclaim_pages(8),
            0,
            "a written page has no other copy, so it is not reclaimable"
        );
        assert!(!not_resident(&vmar, start), "and so it stays resident");
    }

    #[ktest]
    fn a_pages_object_is_left_alone() {
        let vmar = Vmar::new();
        let object = SharedPages::new(PAGE_SIZE).expect("the object is created");
        let backing: Arc<dyn Backing> = Arc::clone(&object) as Arc<dyn Backing>;

        let start = vmar
            .mmap_backed(
                &backing,
                BASE,
                PAGE_SIZE,
                VmPerms::READ | VmPerms::WRITE,
                MmapFlags::SHARED,
                0,
            )
            .expect("the mapping is created");
        vmar.populate_range(&(start..start + PAGE_SIZE))
            .expect("the page faults in");

        assert_eq!(
            vmar.reclaim_pages(8),
            0,
            "the object's pages are not this space's"
        );
        assert_eq!(object.resident_pages(), 1, "and the object still holds it");
        assert!(!not_resident(&vmar, start), "so it stays mapped");
    }

    #[ktest]
    fn the_budget_bounds_a_pass() {
        let vmar = Vmar::new();
        let start = anon(&vmar, 8);
        fault_in_cleanly(&vmar, start, 8);

        assert_eq!(
            vmar.reclaim_pages(3),
            3,
            "a pass takes no more than it was given"
        );
        assert_eq!(
            vmar.reclaim_pages(64),
            5,
            "and finds the rest on the next pass"
        );
        assert_eq!(vmar.reclaim_pages(64), 0, "with nothing left to release");
    }

    #[ktest]
    fn an_empty_address_space_releases_nothing() {
        let vmar = Vmar::new();
        assert_eq!(vmar.reclaim_pages(16), 0);
        assert_eq!(
            vmar.reclaim_pages(0),
            0,
            "and a zero budget asks for nothing"
        );
    }

    #[ktest]
    fn reclaim_reaches_an_address_space_it_was_not_handed() {
        let before_pages = crate::vm::reclaim::pages_reclaimed();
        let before_passes = crate::vm::reclaim::passes();

        let vmar = Vmar::new();
        let start = anon(&vmar, 2);
        fault_in_cleanly(&vmar, start, 2);

        let freed = crate::vm::reclaim::reclaim_at_least(2);
        assert!(freed >= 2, "the clean pages were released, and only they");
        assert!(not_resident(&vmar, start));
        assert!(not_resident(&vmar, start + PAGE_SIZE));
        assert!(crate::vm::reclaim::passes() > before_passes, "a pass ran");
        assert!(
            crate::vm::reclaim::pages_reclaimed() >= before_pages + 2,
            "and it was accounted for"
        );
    }

    #[ktest]
    fn reclaim_at_least_stops_when_there_is_nothing_left() {
        let released = crate::vm::reclaim::reclaim_at_least(1 << 20);
        let passes_after = crate::vm::reclaim::passes();
        let again = crate::vm::reclaim::reclaim_at_least(1 << 20);
        assert_eq!(again, 0);
        assert!(
            crate::vm::reclaim::passes() >= passes_after,
            "and it took at least the one pass that found nothing"
        );
        let _ = released;
    }
}
