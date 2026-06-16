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
        use ostd::mm::io::util::HasVmReaderWriter;
        use ostd::mm::vm_space::VmQueriedItem;

        ostd::early_println!(
            "alloc_frame_for_fault: fault_addr={:#x}, error_code={:?}",
            fault_addr,
            error_code
        );

        let guard = disable_preempt();
        let regions = self.regions.lock();

        let region = regions
            .values()
            .find(|r| r.contains(fault_addr))
            .ok_or_else(|| {
                ostd::early_println!(
                    "alloc_frame_for_fault: region not found for {:#x}",
                    fault_addr
                );
                Error::InvalidArgs
            })?;

        let page_vaddr = fault_addr & !(PAGE_SIZE - 1);
        let vaddr_range = page_vaddr..page_vaddr + PAGE_SIZE;

        let mut cursor = self
            .vm_space
            .cursor_mut(&guard, &vaddr_range)
            .map_err(|e| {
                ostd::early_println!("alloc_frame_for_fault: failed to get cursor: {:?}", e);
                Error::NoMemory
            })?;

        let is_present = error_code.contains(PageFaultErrorCode::PRESENT);
        let is_write = error_code.contains(PageFaultErrorCode::WRITE);

        // If page is not present, allocate a new frame and map it.
        if !is_present {
            ostd::early_println!(
                "alloc_frame_for_fault: page not present at {:#x}, allocating new frame",
                page_vaddr
            );
            let frame: UFrame = FrameAllocOptions::new()
                .zeroed(true)
                .alloc_frame()
                .map_err(|_| Error::NoMemory)?
                .into();
            let property = PageProperty::new_user(region.flags, CachePolicy::Writeback);
            cursor.map(frame, property);

            ostd::early_println!(
                "alloc_frame_for_fault: mapped new frame at {:#x}",
                page_vaddr
            );
            return Ok(());
        }

        // If page is present and has write do Copy on Write
        if is_present && is_write {
            ostd::early_println!(
                "alloc_frame_for_fault: present and write fault at {:#x}, doing COW",
                page_vaddr
            );
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
                ostd::early_println!("alloc_frame_for_fault: ref_count={}", ref_count);

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

                    ostd::early_println!(
                        "alloc_frame_for_fault: COW duplicated frame at {:#x}",
                        page_vaddr
                    );
                    return Ok(());
                } else {
                    let property = PageProperty::new_user(region.flags, CachePolicy::Writeback);
                    cursor.unmap(PAGE_SIZE);
                    cursor.jump(page_vaddr).unwrap();
                    cursor.map(old_frame, property);

                    ostd::early_println!(
                        "alloc_frame_for_fault: COW reused frame with write permissions at {:#x}",
                        page_vaddr
                    );
                    return Ok(());
                }
            }
        }

        ostd::early_println!("alloc_frame_for_fault: unhandled page fault case");
        Err(InvalidArgs)
    }
}

pub fn handle_page_fault(info: &CpuException) -> Result<(), ()> {
    ostd::early_println!("handle_page_fault: exception={:?}", info);
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
}
