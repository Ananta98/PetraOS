use crate::vm::region::VmaRegion;
use crate::vm::{VMA_MANAGER, vma::VmaManager};
use ostd::Error::{self, InvalidArgs};
use ostd::arch::cpu::context::{CpuException, PageFaultErrorCode};
use ostd::mm::io::util::HasVmReaderWriter;
use ostd::mm::vm_space::VmQueriedItem;
use ostd::mm::{CachePolicy, FrameAllocOptions, PAGE_SIZE, PageProperty, UFrame, Vaddr};
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

        // Check if the fault address falls within the guard page region.
        let guard_end = region.start + region.guard_size;
        if region.guard_size > 0 && fault_addr >= region.start && fault_addr < guard_end {
            return Err(Error::InvalidArgs);
        }

        // Clone the region data under the lock, then release the lock before
        // performing the potentially-blocking file read.
        let region_clone = VmaRegion {
            start: region.start,
            size: region.size,
            flags: region.flags,
            guard_size: region.guard_size,
            file_backing: region.file_backing.clone(),
            file_offset: region.file_offset,
            is_shared: region.is_shared,
        };
        drop(regions);

        let page_vaddr = fault_addr & !(PAGE_SIZE - 1);
        let vaddr_range = page_vaddr..page_vaddr + PAGE_SIZE;

        let mut cursor = self
            .vm_space
            .cursor_mut(&guard, &vaddr_range)
            .map_err(|_| Error::NoMemory)?;

        let is_present = error_code.contains(PageFaultErrorCode::PRESENT);
        let is_write = error_code.contains(PageFaultErrorCode::WRITE);

        // If page is not present, allocate a new frame.
        if !is_present {
            let frame: UFrame = FrameAllocOptions::new()
                .zeroed(true)
                .alloc_frame()
                .map_err(|_| Error::NoMemory)?
                .into();

            // If this is a file-backed mapping, read the file data into the frame.
            if let Some(ref backing) = region_clone.file_backing {
                let file_read_offset = region_clone.file_offset + (page_vaddr - region_clone.start);
                let mut frame_writer = frame.writer();
                let mut file_buf = alloc::vec![0u8; PAGE_SIZE];

                if backing.read_at(file_read_offset, &mut file_buf).is_ok() {
                    let mut reader = ostd::mm::io::VmReader::from(&file_buf[..]);
                    frame_writer.write(&mut reader);
                }
                // If read_at fails (e.g., beyond end of file), leave the frame zeroed.
            }

            let property = PageProperty::new_user(region_clone.flags, CachePolicy::Writeback);
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
                let ref_count = old_frame_ref.reference_count();
                let old_frame = (*old_frame_ref).clone();

                if ref_count > 1 {
                    let new_frame: UFrame = FrameAllocOptions::new()
                        .alloc_frame()
                        .map_err(|_| Error::NoMemory)?
                        .into();

                    let mut old_reader = old_frame.reader();
                    let mut new_writer = new_frame.writer();
                    new_writer.write(&mut old_reader);

                    let property =
                        PageProperty::new_user(region_clone.flags, CachePolicy::Writeback);
                    cursor.unmap(PAGE_SIZE);
                    cursor.jump(page_vaddr).unwrap();
                    cursor.map(new_frame, property);

                    return Ok(());
                } else {
                    let property =
                        PageProperty::new_user(region_clone.flags, CachePolicy::Writeback);
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
    use crate::vm::VMA_MANAGER;
    use ostd::mm::PageFlags;
    use ostd::mm::vm_space::VmQueriedItem;
    use ostd::prelude::ktest;

    #[ktest]
    fn test_demand_paging() {
        crate::vm::init();
        let vma_manager = VMA_MANAGER.get().unwrap().clone();
        vma_manager.activate();

        vma_manager
            .map_region_lazy(0x10000, 0x1000, PageFlags::RW)
            .unwrap();

        let data_to_write = b"Lazy Demand Paging Data!";
        vma_manager.copy_to_user(0x10000, data_to_write).unwrap();

        let mut data_read_back = [0u8; 24];
        vma_manager
            .copy_from_user(0x10000, &mut data_read_back)
            .unwrap();
        assert_eq!(data_to_write, &data_read_back);

        vma_manager.unmap_region(0x10000, 0x1000).unwrap();
    }

    #[ktest]
    fn test_cow() {
        crate::vm::init();
        let vma_manager = VMA_MANAGER.get().unwrap().clone();
        vma_manager.activate();

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

        assert_eq!(old_frame.reference_count(), 4);

        let cow_data = b"COW Modified Data!";
        vma_manager.copy_to_user(0x40000, cow_data).unwrap();

        let mut cow_read_back = [0u8; 18];
        vma_manager
            .copy_from_user(0x40000, &mut cow_read_back)
            .unwrap();
        assert_eq!(cow_data, &cow_read_back);

        let mut original_read_back = [0u8; 20];
        vma_manager
            .copy_from_user(0x30000, &mut original_read_back)
            .unwrap();
        assert_eq!(original_data, &original_read_back);

        vma_manager.unmap_region(0x30000, 0x1000).unwrap();
        vma_manager.unmap_region(0x40000, 0x1000).unwrap();
    }
}
