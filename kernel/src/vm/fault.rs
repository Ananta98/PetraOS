use crate::vm::{VMA_MANAGER, vma::VmaManager};
use ostd::Error::{self, InvalidArgs};
use ostd::arch::cpu::context::{CpuException, PageFaultErrorCode};
use ostd::mm::io::util::HasVmReaderWriter;
use ostd::mm::vm_space::VmQueriedItem;
use ostd::mm::{CachePolicy, FrameAllocOptions, PAGE_SIZE, PageFlags, PageProperty, UFrame, Vaddr};
use ostd::task::disable_preempt;

impl VmaManager {
    pub fn alloc_frame_for_fault(
        &self,
        fault_addr: Vaddr,
        error_code: PageFaultErrorCode,
    ) -> Result<(), Error> {
        let guard = disable_preempt();
        let regions = self.regions.lock();

        let region = regions
            .values()
            .find(|r| r.contains(fault_addr))
            .ok_or(Error::InvalidArgs)?;

        let page_vaddr = fault_addr & !(PAGE_SIZE - 1);
        let vaddr_range = page_vaddr..page_vaddr + PAGE_SIZE;

        let mut cursor = self
            .vm_space
            .cursor_mut(&guard, &vaddr_range)
            .map_err(|_| Error::NoMemory)?;

        let is_present = error_code.contains(PageFaultErrorCode::PRESENT);
        let is_write = error_code.contains(PageFaultErrorCode::WRITE);

        // If page is not present, allocate a new frame and map it.
        if !is_present {
            let frame: UFrame = FrameAllocOptions::new()
                .zeroed(true)
                .alloc_frame()
                .map_err(|_| Error::NoMemory)?
                .into();
            let property = PageProperty::new_user(region.flags, CachePolicy::Writeback);
            cursor.map(frame, property);

            return Ok(());
        }

        // If page is present and has write do Copy on Write
        if is_present && is_write {
            let (_range, item) = cursor.query().map_err(|_| InvalidArgs)?;
            let item = item.ok_or(InvalidArgs)?;

            if let VmQueriedItem::MappedRam {
                frame: old_frame_ref,
                prop: _,
            } = item
            {
                // Get the original ref count and clone the frame to release borrow on cursor
                let ref_count = old_frame_ref.reference_count();
                let old_frame = (*old_frame_ref).clone();

                if ref_count > 1 {
                    let new_frame: UFrame = FrameAllocOptions::new()
                        .alloc_frame()
                        .map_err(|_| Error::NoMemory)?
                        .into();

                    // Safe memory copy using OSTD VmReader/VmWriter
                    let mut old_reader = old_frame.reader();
                    let mut new_writer = new_frame.writer();
                    new_writer.write(&mut old_reader);

                    let property = PageProperty::new_user(region.flags, CachePolicy::Writeback);
                    cursor.unmap(PAGE_SIZE);
                    cursor.jump(page_vaddr).unwrap();
                    cursor.map(new_frame, property);

                    return Ok(());
                } else {
                    let property = PageProperty::new_user(region.flags, CachePolicy::Writeback);
                    cursor.unmap(PAGE_SIZE);
                    cursor.jump(page_vaddr).unwrap();
                    cursor.map(old_frame, property);

                    return Ok(());
                }
            }
        }

        Err(InvalidArgs)
    }
}

pub fn handle_page_fault(info: &CpuException) -> Result<(), ()> {
    if let CpuException::PageFault(pf_info) = info {
        if let Some(manager) = VMA_MANAGER.get() {
            if manager
                .alloc_frame_for_fault(pf_info.addr, pf_info.error_code)
                .is_ok()
            {
                return Ok(());
            }
        }
    }
    Err(())
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::mm::vm_space::VmQueriedItem;
    use ostd::prelude::ktest;

    #[ktest]
    fn test_demand_paging() {
        // Initialize the VM system to register page fault handler
        crate::vm::init();
        let vma_manager = crate::vm::VMA_MANAGER.get().unwrap().clone();
        vma_manager.activate();

        vma_manager
            .map_region_lazy(0x10000, 0x1000, PageFlags::RW)
            .unwrap();

        // Write to unallocated page (triggers demand paging page fault)
        let data_to_write = b"Lazy Demand Paging Data!";
        vma_manager.copy_to_user(0x10000, data_to_write).unwrap();

        let mut data_read_back = [0u8; 24];
        vma_manager
            .copy_from_user(0x10000, &mut data_read_back)
            .unwrap();
        assert_eq!(data_to_write, &data_read_back);

        // Clean up
        vma_manager.unmap_region(0x10000, 0x1000).unwrap();
    }

    #[ktest]
    fn test_cow() {
        // Initialize the VM system to register page fault handler
        crate::vm::init();
        let vma_manager = crate::vm::VMA_MANAGER.get().unwrap().clone();
        vma_manager.activate();

        // 1. Allocate a page and write initial data
        vma_manager
            .map_region(0x30000, 0x1000, PageFlags::RW)
            .unwrap();
        let original_data = b"COW Share Test Data!";
        vma_manager.copy_to_user(0x30000, original_data).unwrap();

        let guard = disable_preempt();
        let old_frame = {
            let mut cursor = vma_manager
                .vm_space
                .cursor_mut(&guard, &(0x30000..0x31000))
                .unwrap();
            cursor.jump(0x30000).unwrap();
            let (_range, item) = cursor.query().unwrap();
            let item = item.unwrap();
            if let VmQueriedItem::MappedRam { frame, prop: _ } = item {
                (*frame).clone()
            } else {
                panic!("Expected MappedRam");
            }
        };

        // Simulate sharing the frame: Map the same frame at 0x30000 and 0x40000 as Read-Only
        let mut cursor = vma_manager
            .vm_space
            .cursor_mut(&guard, &(0x30000..0x31000))
            .unwrap();
        cursor.jump(0x30000).unwrap();
        cursor.unmap(0x1000);
        cursor.jump(0x30000).unwrap();
        let ro_property = PageProperty::new_user(PageFlags::R, CachePolicy::Writeback);
        cursor.map(old_frame.clone(), ro_property);
        drop(cursor);

        vma_manager
            .map_region_lazy(0x40000, 0x1000, PageFlags::RW)
            .unwrap();
        let mut cursor2 = vma_manager
            .vm_space
            .cursor_mut(&guard, &(0x40000..0x41000))
            .unwrap();
        cursor2.jump(0x40000).unwrap();
        cursor2.map(old_frame.clone(), ro_property);
        drop(cursor2);

        drop(guard);

        // Ensure refcount is 4 (1 at 0x30000, 1 at 0x40000, 1 in the local `old_frame` variable, and 1 deferred in RCU)
        assert_eq!(old_frame.reference_count(), 4);

        // Write to 0x40000 (triggers COW fault, duplicates frame)
        let cow_data = b"COW Modified Data!";
        vma_manager.copy_to_user(0x40000, cow_data).unwrap();

        // Verify the written page has the modified data
        let mut cow_read_back = [0u8; 18];
        vma_manager
            .copy_from_user(0x40000, &mut cow_read_back)
            .unwrap();
        assert_eq!(cow_data, &cow_read_back);

        // Verify the original page (0x30000) remains unchanged
        let mut original_read_back = [0u8; 20];
        vma_manager
            .copy_from_user(0x30000, &mut original_read_back)
            .unwrap();
        assert_eq!(original_data, &original_read_back);

        // Clean up
        vma_manager.unmap_region(0x30000, 0x1000).unwrap();
        vma_manager.unmap_region(0x40000, 0x1000).unwrap();
    }
}
