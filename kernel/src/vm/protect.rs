use crate::vm::{region::VmaRegion, vma::VmaManager};
use alloc::vec::Vec;
use ostd::{
    Error,
    mm::{CachePolicy, PAGE_SIZE, PageFlags, PageProperty, Vaddr, vm_space::VmQueriedItem},
    task::disable_preempt,
};

impl VmaManager {
    pub fn protect_pages(
        &self,
        start: Vaddr,
        size: usize,
        new_flags: PageFlags,
    ) -> Result<(), Error> {
        let guard = disable_preempt();
        let vaddr_range = start..start + size;

        let mut cursor = self
            .vm_space
            .cursor_mut(&guard, &vaddr_range)
            .map_err(|_| Error::InvalidArgs)?;

        let num_pages = size / PAGE_SIZE;

        for page_idx in 0..num_pages {
            let page_vaddr = start + (page_idx * PAGE_SIZE);
            cursor.jump(page_vaddr).map_err(|_| Error::InvalidArgs)?;

            let (_, item) = cursor.query().map_err(|_| Error::InvalidArgs)?;
            if let Some(VmQueriedItem::MappedRam { frame, prop: _ }) = item {
                let frame_ref = (*frame).clone();
                let property = PageProperty::new_user(new_flags, CachePolicy::Writeback);

                cursor.unmap(PAGE_SIZE);
                cursor.jump(page_vaddr).unwrap();
                cursor.map(frame_ref, property);
            } else {
                cursor.unmap(PAGE_SIZE);
            }
        }

        Ok(())
    }

    pub fn mprotect(&self, start: Vaddr, size: usize, new_flags: PageFlags) -> Result<(), Error> {
        if start % PAGE_SIZE != 0 || size % PAGE_SIZE != 0 {
            return Err(Error::InvalidArgs);
        }

        let start_addr = start;
        let end_addr = start + size;

        let mut regions = self.regions.lock();

        // 1. Find overlapping VMAs
        let mut overlapping_vmas = Vec::new();
        for (_, region) in regions.iter() {
            let region_end = region.start + region.size;
            if region.start >= end_addr || region_end <= start_addr {
                continue; // No overlap
            }
            overlapping_vmas.push(VmaRegion::new(region.start, region.size, region.flags));
        }

        if overlapping_vmas.is_empty() {
            return Err(Error::NoMemory);
        }

        for vma in overlapping_vmas {
            let end = vma.start + vma.size;
            regions.remove(&vma.start);

            if start_addr <= vma.start && end_addr >= end {
                // Case 1: Target fully covers the VMA
                regions.insert(vma.start, VmaRegion::new(vma.start, vma.size, new_flags));
            } else if vma.start < start_addr && end > end_addr {
                // Case 2: Target is completely inside the VMA (splits into 3)
                regions.insert(
                    vma.start,
                    VmaRegion::new(vma.start, start_addr - vma.start, vma.flags),
                );
                regions.insert(start_addr, VmaRegion::new(start_addr, size, new_flags));
                regions.insert(
                    end_addr,
                    VmaRegion::new(end_addr, end - end_addr, vma.flags),
                );
            } else if vma.start < start_addr && end <= end_addr {
                // Case 3: Target overlaps the right side of the VMA
                regions.insert(
                    vma.start,
                    VmaRegion::new(vma.start, start_addr - vma.start, vma.flags),
                );
                regions.insert(
                    start_addr,
                    VmaRegion::new(start_addr, end - start_addr, new_flags),
                );
            } else if vma.start >= start_addr && end > end_addr {
                // Case 4: Target overlaps the left side of the VMA
                regions.insert(
                    vma.start,
                    VmaRegion::new(vma.start, end_addr - vma.start, new_flags),
                );
                regions.insert(
                    end_addr,
                    VmaRegion::new(end_addr, end - end_addr, vma.flags),
                );
            }
        }

        self.protect_pages(start, size, new_flags)
    }
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use crate::vm::VMA_MANAGER;
    use alloc::sync::Arc;
    use ostd::prelude::ktest;

    #[ktest]
    fn test_mprotect_perfect_match() {
        crate::vm::init();
        let vma_manager = VMA_MANAGER.get().unwrap().clone();
        vma_manager.activate();

        // Map a 1-page RW region
        vma_manager
            .map_region(0x60000, PAGE_SIZE, PageFlags::RW)
            .unwrap();

        // Change to Read-Only
        vma_manager
            .mprotect(0x60000, PAGE_SIZE, PageFlags::R)
            .unwrap();

        // Verify metadata
        let regions = vma_manager.regions.lock();
        let region = regions.get(&0x60000).unwrap();
        assert_eq!(region.flags, PageFlags::R);
        assert_eq!(region.size, PAGE_SIZE);
        drop(regions);

        vma_manager.unmap_region(0x60000, PAGE_SIZE).unwrap();
    }

    #[ktest]
    fn test_mprotect_split_middle() {
        crate::vm::init();
        let vma_manager = VMA_MANAGER.get().unwrap().clone();
        vma_manager.activate();

        // Map a 3-page RW region from 0x70000 to 0x73000
        vma_manager
            .map_region(0x70000, PAGE_SIZE * 3, PageFlags::RW)
            .unwrap();

        // Protect only the middle page (0x71000) as Read-Only
        vma_manager
            .mprotect(0x71000, PAGE_SIZE, PageFlags::R)
            .unwrap();

        // Verify split regions: [0x70000: RW, size=1], [0x71000: R, size=1], [0x72000: RW, size=1]
        let regions = vma_manager.regions.lock();
        assert_eq!(regions.len(), 3);

        let r1 = regions.get(&0x70000).unwrap();
        assert_eq!(r1.flags, PageFlags::RW);
        assert_eq!(r1.size, PAGE_SIZE);

        let r2 = regions.get(&0x71000).unwrap();
        assert_eq!(r2.flags, PageFlags::R);
        assert_eq!(r2.size, PAGE_SIZE);

        let r3 = regions.get(&0x72000).unwrap();
        assert_eq!(r3.flags, PageFlags::RW);
        assert_eq!(r3.size, PAGE_SIZE);

        drop(regions);
        vma_manager.unmap_region(0x70000, PAGE_SIZE).unwrap();
        vma_manager.unmap_region(0x71000, PAGE_SIZE).unwrap();
        vma_manager.unmap_region(0x72000, PAGE_SIZE).unwrap();
    }

    #[ktest]
    fn test_mprotect_split_left_and_right() {
        crate::vm::init();
        let vma_manager = VMA_MANAGER.get().unwrap().clone();
        vma_manager.activate();

        // Map a 2-page RW region from 0x80000 to 0x82000
        vma_manager
            .map_region(0x80000, PAGE_SIZE * 2, PageFlags::RW)
            .unwrap();

        // Protect the first page (overlaps left side of VMA)
        vma_manager
            .mprotect(0x80000, PAGE_SIZE, PageFlags::R)
            .unwrap();

        let regions = vma_manager.regions.lock();
        assert_eq!(regions.len(), 2);
        assert_eq!(regions.get(&0x80000).unwrap().flags, PageFlags::R);
        assert_eq!(regions.get(&0x81000).unwrap().flags, PageFlags::RW);
        drop(regions);

        vma_manager.unmap_region(0x80000, PAGE_SIZE).unwrap();
        vma_manager.unmap_region(0x81000, PAGE_SIZE).unwrap();
    }
}
