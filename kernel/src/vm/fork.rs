use crate::vm::region::VmaRegion;
use crate::vm::vma::VmaManager;
use alloc::sync::Arc;
use ostd::Error;
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
                    guard_size: region.guard_size,
                    file_backing: region.file_backing.clone(),
                    file_offset: region.file_offset,
                    is_shared: region.is_shared,
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
    use ostd::arch::cpu::context::PageFaultErrorCode;
    use ostd::mm::HasPaddr;
    use ostd::prelude::ktest;

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

        let guard = disable_preempt();

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
            assert_eq!(prop.flags, PageFlags::R);
            (*frame).clone()
        };

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
            assert_eq!(prop.flags, PageFlags::R);
            (*frame).clone()
        };

        assert_eq!(parent_frame.paddr(), child_frame.paddr());
        assert_eq!(parent_frame.reference_count(), 5);

        drop(guard);

        child_manager
            .alloc_frame_for_fault(
                0x50000,
                PageFaultErrorCode::PRESENT | PageFaultErrorCode::WRITE,
            )
            .unwrap();

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
            assert_eq!(prop.flags, PageFlags::RW);
            (*frame).clone()
        };

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
            assert_eq!(prop.flags, PageFlags::R);
            (*frame).clone()
        };

        assert_ne!(
            parent_frame_after_fault.paddr(),
            child_frame_after_fault.paddr()
        );
        assert_eq!(parent_frame_after_fault.paddr(), parent_frame.paddr());

        drop(guard2);

        child_manager.unmap_region(0x50000, 0x1000).unwrap();
        parent_manager.unmap_region(0x50000, 0x1000).unwrap();
    }
}
