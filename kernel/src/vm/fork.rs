use crate::vm::region::VmaRegion;
use crate::vm::vma::VmaManager;
use alloc::sync::Arc;
use ostd::Error;
use ostd::arch::cpu::context::PageFaultErrorCode;
use ostd::mm::vm_space::VmQueriedItem;
use ostd::mm::{CachePolicy, PAGE_SIZE, PageFlags, PageProperty};
use ostd::task::disable_preempt;

impl VmaManager {
    pub fn fork_vm_space(&self) -> Result<Arc<Self>, Error> {
        let guard = disable_preempt();
        let child_manager = Arc::new(VmaManager::new());

        let parent_regions = self.regions.lock();
        let mut child_regions = child_manager.regions.lock();

        for (start, region) in parent_regions.iter() {
            child_regions.insert(
                *start,
                VmaRegion {
                    start: region.start,
                    size: region.size,
                    flags: region.flags,
                },
            );

            let num_pages = region.size / PAGE_SIZE;
            for page_idx in 0..num_pages {
                let page_vaddr = region.start + (page_idx * PAGE_SIZE);
                let vaddr_range = page_vaddr..page_vaddr + PAGE_SIZE;

                let mut parent_cursor = self
                    .vm_space
                    .cursor_mut(&guard, &vaddr_range)
                    .map_err(|_| Error::NoMemory)?;
                parent_cursor
                    .jump(page_vaddr)
                    .map_err(|_| Error::InvalidArgs)?;

                let (_range, item) = parent_cursor.query().map_err(|_| Error::InvalidArgs)?;
                if let Some(VmQueriedItem::MappedRam { frame, prop: _ }) = item {
                    let old_frame = (*frame).clone();

                    let ro_property = PageProperty::new_user(PageFlags::R, CachePolicy::Writeback);
                    parent_cursor.unmap(PAGE_SIZE);
                    parent_cursor.jump(page_vaddr).unwrap();
                    parent_cursor.map(old_frame.clone(), ro_property);

                    let mut child_cursor = child_manager
                        .vm_space
                        .cursor_mut(&guard, &vaddr_range)
                        .map_err(|_| Error::NoMemory)?;
                    child_cursor
                        .jump(page_vaddr)
                        .map_err(|_| Error::InvalidArgs)?;
                    child_cursor.map(old_frame, ro_property);
                }
            }
        }

        drop(child_regions);
        drop(parent_regions);
        drop(guard);

        Ok(child_manager)
    }
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::mm::HasPaddr;
    use ostd::prelude::ktest;

    #[ktest]
    fn test_fork_cow() {
        // Initialize VM system
        crate::vm::init();
        let parent_manager = crate::vm::VMA_MANAGER.get().unwrap().clone();
        parent_manager.activate();

        // 1. Map parent region and write parent data
        parent_manager
            .map_region(0x50000, 0x1000, PageFlags::RW)
            .unwrap();
        let original_data = b"Fork Parent Data!";
        parent_manager.copy_to_user(0x50000, original_data).unwrap();

        // 2. Fork VM space
        let child_manager = parent_manager.fork_vm_space().unwrap();

        // 3. Inspect page table state under guard
        let guard = disable_preempt();

        // Parent query
        let parent_frame = {
            let mut cursor = parent_manager
                .vm_space
                .cursor_mut(&guard, &(0x50000..0x51000))
                .unwrap();
            cursor.jump(0x50000).unwrap();
            let (_range, item) = cursor.query().unwrap();
            let VmQueriedItem::MappedRam { frame, prop } = item.unwrap() else {
                panic!("Expected MappedRam in parent");
            };
            // Parent should be Read-Only now after fork!
            assert_eq!(prop.flags, PageFlags::R);
            (*frame).clone()
        };

        // Child query
        let child_frame = {
            let mut cursor = child_manager
                .vm_space
                .cursor_mut(&guard, &(0x50000..0x51000))
                .unwrap();
            cursor.jump(0x50000).unwrap();
            let (_range, item) = cursor.query().unwrap();
            let VmQueriedItem::MappedRam { frame, prop } = item.unwrap() else {
                panic!("Expected MappedRam in child");
            };
            // Child should also be Read-Only!
            assert_eq!(prop.flags, PageFlags::R);
            (*frame).clone()
        };

        // Parent and child must point to the same physical frame
        assert_eq!(parent_frame.paddr(), child_frame.paddr());

        // Frame reference count must be 5:
        // 1 in parent pt, 1 in child pt, 1 in parent_frame variable, 1 in child_frame variable,
        // and 1 queued in the RCU grace period deferred drop queue from the parent's unmap step.
        assert_eq!(parent_frame.reference_count(), 5);

        drop(guard);

        // 4. Manually trigger the COW fault allocation on the child
        child_manager
            .alloc_frame_for_fault(
                0x50000,
                PageFaultErrorCode::PRESENT | PageFaultErrorCode::WRITE,
            )
            .unwrap();

        // 5. Verify child's mapping after fault
        let guard2 = disable_preempt();
        let child_frame_after_fault = {
            let mut cursor = child_manager
                .vm_space
                .cursor_mut(&guard2, &(0x50000..0x51000))
                .unwrap();
            cursor.jump(0x50000).unwrap();
            let (_range, item) = cursor.query().unwrap();
            let VmQueriedItem::MappedRam { frame, prop } = item.unwrap() else {
                panic!("Expected MappedRam in child after fault");
            };
            // Child should now be RW!
            assert_eq!(prop.flags, PageFlags::RW);
            (*frame).clone()
        };

        // Parent should still be pointing to the original frame
        let parent_frame_after_fault = {
            let mut cursor = parent_manager
                .vm_space
                .cursor_mut(&guard2, &(0x50000..0x51000))
                .unwrap();
            cursor.jump(0x50000).unwrap();
            let (_range, item) = cursor.query().unwrap();
            let VmQueriedItem::MappedRam { frame, prop } = item.unwrap() else {
                panic!("Expected MappedRam in parent after fault");
            };
            // Parent remains Read-Only
            assert_eq!(prop.flags, PageFlags::R);
            (*frame).clone()
        };

        // They must point to DIFFERENT frames now!
        assert_ne!(
            parent_frame_after_fault.paddr(),
            child_frame_after_fault.paddr()
        );
        assert_eq!(parent_frame_after_fault.paddr(), parent_frame.paddr());

        drop(guard2);

        // Clean up child and parent regions
        child_manager.unmap_region(0x50000, 0x1000).unwrap();
        parent_manager.unmap_region(0x50000, 0x1000).unwrap();
    }
}
