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
