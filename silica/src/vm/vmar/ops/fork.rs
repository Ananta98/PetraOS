// SPDX-License-Identifier: GPL-2.0

//! `fork(2)` address space duplication and Copy-on-Write sharing.

use alloc::sync::Arc;
use core::sync::atomic::Ordering;
use ostd::mm::{
    CachePolicy, PageFlags,
    tlb::TlbFlushOp,
    vm_space::{CursorMut, VmQueriedItem},
};
use ostd::task::disable_preempt;

use super::super::{VMAR_CAP_ADDR, Vmar};

impl Vmar {
    /// Creates an address space that starts out as an image of `parent`.
    pub fn fork_from(parent: &Arc<Vmar>) -> Arc<Vmar> {
        let child = Vmar::new();
        child
            .max_addr_space
            .store(parent.max_addr_space().unwrap_or(0), Ordering::Relaxed);
        child.start_brk.store(parent.start_brk(), Ordering::Relaxed);
        child
            .current_brk
            .store(parent.current_brk(), Ordering::Relaxed);

        let guard = disable_preempt();
        let whole = 0..VMAR_CAP_ADDR;
        let Ok(mut parent_cursor) = parent.vm_space().cursor_mut(&guard, &whole) else {
            return child;
        };

        {
            let parent_inner = parent.inner.read();
            let mut child_inner = child.inner.write();

            for mapping in parent_inner.mappings.iter() {
                // If marked with MADV_DONTFORK, do not inherit in child
                if mapping.dont_fork() {
                    continue;
                }

                let range = mapping.range();
                let is_cow = mapping.is_cow();

                // If marked with MADV_WIPEONFORK, child gets a clean zeroed mapping
                if mapping.wipe_on_fork() {
                    let mut fresh_mapping = mapping.dup();
                    fresh_mapping.set_wipe_on_fork(false);
                    child_inner.total_vm += fresh_mapping.len();
                    child_inner.mappings.insert(fresh_mapping);
                    continue;
                }

                child_inner.total_vm += mapping.len();
                child_inner.mappings.insert(mapping.dup());

                let Ok(mut child_cursor) = child.vm_space().cursor_mut(&guard, &range) else {
                    continue;
                };
                parent_cursor.jump(range.start).expect("page-aligned");
                child_cursor.jump(range.start).expect("page-aligned");
                Self::share_pages(&mut parent_cursor, &mut child_cursor, range.len(), is_cow);
            }
        }

        parent_cursor
            .flusher()
            .issue_tlb_flush(TlbFlushOp::for_all());
        parent_cursor.flusher().dispatch_tlb_flush();
        parent_cursor.flusher().sync_tlb_flush();
        child
    }

    /// Shares resident pages from `src` into `dst`.
    ///
    /// For CoW mappings (`is_cow == true`), pages are marked read-only in both parent and child.
    /// For shared mappings (`is_cow == false`), pages remain writable in both to avoid spurious write traps.
    fn share_pages(
        src: &mut CursorMut<'_>,
        dst: &mut CursorMut<'_>,
        size: usize,
        is_cow: bool,
    ) -> usize {
        let end = src.virt_addr() + size;
        let mut shared = 0;

        while let Some(mapped) = src.find_next(end - src.virt_addr()) {
            let (_, Some(item)) = src.query().expect("a mapped page was just found") else {
                break;
            };
            match item {
                VmQueriedItem::MappedRam { frame, mut prop } => {
                    let frame = (*frame).clone();

                    if is_cow {
                        src.protect_next(end - mapped, make_read_only)
                            .expect("a mapped page was just found");
                        make_read_only(&mut prop.flags, &mut prop.cache);
                    } else {
                        // Shared mapping: advance cursor and keep writable
                        src.jump(mapped + ostd::mm::PAGE_SIZE).ok();
                    }

                    dst.jump(mapped).expect("page-aligned");
                    dst.map(frame, prop);
                    shared += 1;
                }
                VmQueriedItem::MappedIoMem { .. } => {
                    unreachable!("device mappings are not supported")
                }
            }
        }
        shared
    }
}

/// Clears the write bit, turning a shared page into a CoW page.
fn make_read_only(flags: &mut PageFlags, _cache: &mut CachePolicy) {
    *flags -= PageFlags::W;
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use crate::vm::abi::{flags::MmapFlags, perms::VmPerms};
    use crate::vm::backing::Backing;
    use ostd::mm::PAGE_SIZE;
    use ostd::prelude::ktest;

    #[ktest]
    fn repro_fork_shared_mapping_remains_writable() {
        let parent = Vmar::new();
        let shm = crate::vm::backing::shm::new_anonymous(PAGE_SIZE).unwrap();
        let backing: alloc::sync::Arc<dyn Backing> = shm;
        let addr = parent
            .mmap_backed(
                &backing,
                0x5000_0000,
                PAGE_SIZE,
                VmPerms::READ | VmPerms::WRITE,
                MmapFlags::SHARED | MmapFlags::FIXED,
                0,
            )
            .unwrap();

        parent.populate_range(&(addr..addr + PAGE_SIZE)).unwrap();
        let child = Vmar::fork_from(&parent);

        let parent_flags = crate::vm::tests::page_at(&parent, addr).unwrap().1;
        let child_flags = crate::vm::tests::page_at(&child, addr).unwrap().1;

        assert!(
            parent_flags.contains(ostd::mm::PageFlags::W),
            "parent shared page must remain writable"
        );
        assert!(
            child_flags.contains(ostd::mm::PageFlags::W),
            "child shared page must remain writable"
        );
    }
}
