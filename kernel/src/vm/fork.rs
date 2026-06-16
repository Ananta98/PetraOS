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
                },
            );

            let num_pages = region.size / PAGE_SIZE;
            for page_idx in 0..num_pages {
                let page_vaddr = region.start + (page_idx * PAGE_SIZE);
                let vaddr_range = page_vaddr..page_vaddr + PAGE_SIZE;

                // Open cursor in parent VM space
                let mut parent_cursor = self
                    .vm_space
                    .cursor_mut(&guard, &vaddr_range)
                    .map_err(|_| Error::NoMemory)?;
                parent_cursor
                    .jump(page_vaddr)
                    .map_err(|_| Error::InvalidArgs)?;

                // Query parent's mapping
                let (_range, item) = parent_cursor.query().map_err(|_| Error::InvalidArgs)?;
                if let Some(VmQueriedItem::MappedRam { frame, prop: _ }) = item {
                    let old_frame = (*frame).clone();

                    // Unmap parent's current mapping and map back as Read-Only (COW)
                    let ro_property = PageProperty::new_user(PageFlags::R, CachePolicy::Writeback);
                    parent_cursor.unmap(PAGE_SIZE);
                    parent_cursor.jump(page_vaddr).unwrap();
                    parent_cursor.map(old_frame.clone(), ro_property);

                    // Map in child as Read-Only
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
    use ostd::prelude::ktest;

    #[ktest]
    fn test_fork_cow() {
        // Initialize VM system
        crate::vm::init();
        let parent_manager = crate::vm::VMA_MANAGER.get().unwrap().clone();
        parent_manager.activate();

        // Map parent region and write parent data
        parent_manager
            .map_region(0x50000, 0x1000, PageFlags::RW)
            .unwrap();
        let parent_data = b"Parent Shared Data!";
        parent_manager.copy_to_user(0x50000, parent_data).unwrap();

        // Fork VM space
        let child_manager = parent_manager.fork_vm_space().unwrap();

        // Activate child
        child_manager.activate();

        // Verify child can read parent's data
        let mut child_read_back = [0u8; 19];
        child_manager
            .copy_from_user(0x50000, &mut child_read_back)
            .unwrap();
        assert_eq!(parent_data, &child_read_back);

        // Write child data (triggers COW in child)
        let child_data = b"Child Modified Data";
        child_manager.copy_to_user(0x50000, child_data).unwrap();

        // Verify child reads child data
        let mut child_read_modified = [0u8; 19];
        child_manager
            .copy_from_user(0x50000, &mut child_read_modified)
            .unwrap();
        assert_eq!(child_data, &child_read_modified);

        // Activate parent again
        parent_manager.activate();

        // Verify parent still reads original parent data
        let mut parent_read_back = [0u8; 19];
        parent_manager
            .copy_from_user(0x50000, &mut parent_read_back)
            .unwrap();
        assert_eq!(parent_data, &parent_read_back);

        // Clean up child and parent regions
        child_manager.unmap_region(0x50000, 0x1000).unwrap();
        parent_manager.unmap_region(0x50000, 0x1000).unwrap();
    }
}
