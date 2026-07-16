//! Copy-on-Write (CoW) fork implementation for virtual memory spaces.
//!
//! This module implements [`VmaManager::fork_vm_space`], which creates a child
//! address space from a parent during a `fork()`-like operation. Rather than
//! duplicating physical frames immediately, both parent and child initially share
//! the same physical frames mapped as **read-only**. A new private frame is only
//! allocated when one of them actually writes to the page — the classic
//! Copy-on-Write strategy.
//!
//! # CoW Lifecycle
//!
//! ```text
//!   fork()
//!     │
//!     ├─ parent frame → remapped R/O in parent page table
//!     └─ child frame  → shared with parent, also R/O
//!
//!   write fault (in child or parent)
//!     └─ alloc_frame_for_fault() → allocates a new private frame, restores R/W
//! ```

use crate::vm::vma::VmaManager;
use alloc::sync::Arc;
use ostd::Error;
use ostd::mm::vm_space::VmQueriedItem;
use ostd::mm::{CachePolicy, PAGE_SIZE, PageFlags, PageProperty};
use ostd::task::disable_preempt;

impl VmaManager {
    /// Creates a child [`VmaManager`] that shares physical frames with this (parent) manager
    /// using Copy-on-Write semantics.
    ///
    /// All currently mapped pages in the parent are:
    /// 1. Remapped as **read-only** in the parent's page table.
    /// 2. Shared (read-only) into the child's page table pointing to the same frame.
    ///
    /// Actual frame copying is deferred until a write fault occurs — handled by
    /// [`VmaManager::alloc_frame_for_fault`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoMemory`] if a page-table cursor cannot be created, or
    /// [`Error::InvalidArgs`] if cursor navigation or querying fails.
    pub fn fork_vm_space(&self) -> Result<Arc<Self>, Error> {
        let guard = disable_preempt();
        let child_manager = Arc::new(VmaManager::new());

        // Copy the heap-break state atomically under a single lock.
        *child_manager.brk.lock() = *self.brk.lock();

        let parent_regions = self.regions.lock();
        let mut child_regions = child_manager.regions.lock();

        for (start_vaddr, region) in parent_regions.iter() {
            // Mirror the region metadata into the child's region map.
            child_regions.insert(*start_vaddr, region.clone());

            // Iterate over every page in the region and establish a shared
            // CoW mapping between parent and child for each backed page.
            let num_pages = region.size / PAGE_SIZE;
            for page_index in 0..num_pages {
                let page_vaddr = region.start + (page_index * PAGE_SIZE);
                let vaddr_range = page_vaddr..page_vaddr + PAGE_SIZE;

                self.setup_cow_page(&child_manager, &guard, page_vaddr, &vaddr_range)?;
            }
        }

        // Explicit drops clarify the lock-release order for future readers.
        drop(child_regions);
        drop(parent_regions);
        drop(guard);

        Ok(child_manager)
    }

    /// Converts a single mapped page into a CoW-shared page between parent and child.
    ///
    /// If the page at `page_vaddr` is backed by a RAM frame:
    /// - The parent's mapping is replaced with a **read-only** copy of the same frame.
    /// - The child receives an identical **read-only** mapping to that same frame.
    ///
    /// If the page is not yet mapped, this function is a no-op for that page.
    ///
    /// # Parameters
    ///
    /// - `child`: The child [`VmaManager`] being constructed.
    /// - `guard`: The preemption-disable guard required by `cursor_mut`.
    /// - `page_vaddr`: The virtual address of the page to CoW-share.
    /// - `vaddr_range`: A `page_vaddr..page_vaddr + PAGE_SIZE` range used for
    ///   cursor creation — passed in to avoid redundant computation.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoMemory`] if a page-table cursor cannot be created, or
    /// [`Error::InvalidArgs`] if cursor navigation or querying fails.
    fn setup_cow_page(
        &self,
        child: &VmaManager,
        guard: &ostd::task::DisabledPreemptGuard,
        page_vaddr: usize,
        vaddr_range: &core::ops::Range<usize>,
    ) -> Result<(), Error> {
        let mut parent_cursor = self
            .vm_space
            .cursor_mut(guard, vaddr_range)
            .map_err(|_| Error::NoMemory)?;

        parent_cursor
            .jump(page_vaddr)
            .map_err(|_| Error::InvalidArgs)?;

        let (_range, queried_item) = parent_cursor.query().map_err(|_| Error::InvalidArgs)?;

        // Only pages currently backed by a RAM frame are CoW-shared;
        // unmapped or non-RAM pages are skipped.
        let Some(VmQueriedItem::MappedRam { frame, prop: _ }) = queried_item else {
            return Ok(());
        };

        let shared_frame = (*frame).clone();
        let read_only_property = PageProperty::new_user(PageFlags::R, CachePolicy::Writeback);

        // Remap the parent's page as read-only so that any subsequent write
        // in the parent also triggers a CoW fault.
        parent_cursor.unmap(PAGE_SIZE);
        parent_cursor
            .jump(page_vaddr)
            .map_err(|_| Error::InvalidArgs)?;
        parent_cursor.map(shared_frame.clone(), read_only_property);

        // Map the same physical frame into the child's address space, also read-only.
        let mut child_cursor = child
            .vm_space
            .cursor_mut(guard, vaddr_range)
            .map_err(|_| Error::NoMemory)?;

        child_cursor
            .jump(page_vaddr)
            .map_err(|_| Error::InvalidArgs)?;

        child_cursor.map(shared_frame, read_only_property);

        Ok(())
    }
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::arch::cpu::context::PageFaultErrorCode;
    use ostd::mm::HasPaddr;
    use ostd::prelude::ktest;

    /// Verifies the full CoW lifecycle across a `fork_vm_space` call:
    ///
    /// 1. Immediately after `fork_vm_space`, parent and child share the same physical frame, both R/O.
    /// 2. After a write fault in the child, it receives its own private frame (R/W).
    /// 3. The parent's original frame and flags remain unchanged.
    #[ktest]
    fn test_fork_cow() {
        crate::vm::init();
        let parent_manager = crate::vm::VMA_MANAGER.get().unwrap().clone();
        parent_manager.activate();

        parent_manager
            .map_region(0x50000, 0x1000, PageFlags::RW)
            .unwrap();
        let original_data = b"Fork Parent Data!";
        parent_manager.copy_to_user(0x50000, original_data).unwrap();

        let child_manager = parent_manager.fork_vm_space().unwrap();

        // ── Phase 1: shared R/O frame ──────────────────────────────────────────
        let guard = disable_preempt();

        let parent_frame = query_single_frame(&parent_manager, &guard, 0x50000, PageFlags::R);
        let child_frame = query_single_frame(&child_manager, &guard, 0x50000, PageFlags::R);

        assert_eq!(
            parent_frame.paddr(),
            child_frame.paddr(),
            "parent and child must share the same physical frame after fork"
        );
        // Reference count: 2 from the test locals + 2 page-table entries + 1 internal.
        assert_eq!(parent_frame.reference_count(), 5);

        drop(guard);

        // ── Phase 2: CoW write fault in child allocates a new private frame ────
        child_manager
            .alloc_frame_for_fault(
                0x50000,
                PageFaultErrorCode::PRESENT | PageFaultErrorCode::WRITE,
            )
            .unwrap();

        let guard2 = disable_preempt();

        let child_frame_after_fault =
            query_single_frame(&child_manager, &guard2, 0x50000, PageFlags::RW);
        let parent_frame_after_fault =
            query_single_frame(&parent_manager, &guard2, 0x50000, PageFlags::R);

        assert_ne!(
            parent_frame_after_fault.paddr(),
            child_frame_after_fault.paddr(),
            "child must have a distinct frame after a CoW write fault"
        );
        assert_eq!(
            parent_frame_after_fault.paddr(),
            parent_frame.paddr(),
            "parent frame must not change after child's CoW fault"
        );

        drop(guard2);

        child_manager.unmap_region(0x50000, 0x1000).unwrap();
        parent_manager.unmap_region(0x50000, 0x1000).unwrap();
    }

    /// Queries the mapped RAM frame at `vaddr` and asserts the expected page flags.
    ///
    /// # Panics
    ///
    /// Panics if no `MappedRam` item is found at `vaddr`, or if the flags mismatch.
    fn query_single_frame(
        manager: &VmaManager,
        guard: &ostd::task::DisabledPreemptGuard,
        vaddr: usize,
        expected_flags: PageFlags,
    ) -> ostd::mm::UFrame {
        let vaddr_range = vaddr..vaddr + PAGE_SIZE;
        let mut cursor = manager
            .vm_space
            .cursor_mut(guard, &vaddr_range)
            .expect("cursor creation must not fail");
        cursor.jump(vaddr).expect("cursor jump must not fail");

        let (_range, item) = cursor.query().expect("cursor query must not fail");
        let VmQueriedItem::MappedRam { frame, prop } =
            item.expect("page must be backed by a frame")
        else {
            panic!("Expected MappedRam at {:#x}", vaddr);
        };
        assert_eq!(
            prop.flags, expected_flags,
            "page flags mismatch at {:#x}",
            vaddr
        );
        (*frame).clone()
    }
}
