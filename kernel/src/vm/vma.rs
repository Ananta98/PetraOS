use crate::vm::region::VmaRegion;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use ostd::Error::{self};
use ostd::mm::io::{FallibleVmRead, FallibleVmWrite, VmReader, VmWriter};
use ostd::mm::vm_space::VmSpace;
use ostd::mm::{CachePolicy, FrameAllocOptions, PAGE_SIZE, PageFlags, PageProperty, UFrame, Vaddr};
use ostd::sync::SpinLock;
use ostd::task::disable_preempt;

pub struct VmaManager {
    pub vm_space: Arc<VmSpace>,
    pub regions: SpinLock<BTreeMap<Vaddr, VmaRegion>>,
}

impl VmaManager {
    pub fn new() -> Self {
        Self {
            vm_space: Arc::new(VmSpace::new()),
            regions: SpinLock::new(BTreeMap::new()),
        }
    }

    pub fn map_region(&self, start: Vaddr, size: usize, flags: PageFlags) -> Result<(), Error> {
        if start % PAGE_SIZE != 0 || size % PAGE_SIZE != 0 {
            return Err(Error::InvalidArgs);
        }
        let guard = disable_preempt();
        let vaddr_range = start..start + size;
        let mut cursor = self
            .vm_space
            .cursor_mut(&guard, &vaddr_range)
            .map_err(|_| Error::NoMemory)?;

        let property = PageProperty::new_user(flags, CachePolicy::Writeback);
        let num_pages = size / PAGE_SIZE;

        for page_idx in 0..num_pages {
            let page_vaddr = start + (page_idx * PAGE_SIZE);
            cursor.jump(page_vaddr).map_err(|_| Error::InvalidArgs)?;
            let frame: UFrame = FrameAllocOptions::new()
                .zeroed(true)
                .alloc_frame()
                .map_err(|_| Error::NoMemory)?
                .into();
            cursor.map(frame, property);
        }

        let mut regions = self.regions.lock();
        regions.insert(
            start,
            VmaRegion {
                start,
                size,
                flags,
                guard_size: 0,
            },
        );

        Ok(())
    }

    pub fn map_stack(
        &self,
        start: Vaddr,
        stack_size: usize,
        guard_size: usize,
    ) -> Result<(), Error> {
        if start % PAGE_SIZE != 0 || stack_size % PAGE_SIZE != 0 || guard_size % PAGE_SIZE != 0 {
            return Err(Error::InvalidArgs);
        }

        let total_size = stack_size + guard_size;
        let mut regions = self.regions.lock();
        regions.insert(
            start,
            VmaRegion {
                start,
                size: total_size,
                flags: PageFlags::RW,
                guard_size,
            },
        );
        Ok(())
    }

    pub fn unmap_region(&self, start: Vaddr, size: usize) -> Result<(), Error> {
        if start % PAGE_SIZE != 0 || size % PAGE_SIZE != 0 {
            return Err(Error::InvalidArgs);
        }

        let guard = disable_preempt();
        let vaddr_range = start..start + size;
        let mut cursor = self
            .vm_space
            .cursor_mut(&guard, &vaddr_range)
            .map_err(|_| Error::NoMemory)?;

        cursor.unmap(size);

        let mut regions = self.regions.lock();
        regions.remove(&start);

        Ok(())
    }

    pub fn map_region_lazy(
        &self,
        start: Vaddr,
        size: usize,
        flags: PageFlags,
    ) -> Result<(), Error> {
        if start % PAGE_SIZE != 0 || size % PAGE_SIZE != 0 {
            return Err(Error::InvalidArgs);
        }

        let mut regions = self.regions.lock();
        regions.insert(
            start,
            VmaRegion {
                start,
                size,
                flags,
                guard_size: 0,
            },
        );

        Ok(())
    }

    pub fn activate(self: &Arc<Self>) {
        self.vm_space.activate();
    }

    pub fn copy_from_user(&self, user_src: Vaddr, kernel_dest: &mut [u8]) -> Result<(), Error> {
        let len = kernel_dest.len();
        let mut writer = VmWriter::from(kernel_dest);
        let mut reader = self.vm_space.reader(user_src, len)?;
        reader.read_fallible(&mut writer).map_err(|(err, _)| err)?;
        Ok(())
    }

    pub fn copy_to_user(&self, user_dest: Vaddr, kernel_src: &[u8]) -> Result<(), Error> {
        let len = kernel_src.len();
        let mut reader = VmReader::from(kernel_src);
        let mut writer = self.vm_space.writer(user_dest, len)?;
        writer.write_fallible(&mut reader).map_err(|(err, _)| err)?;
        Ok(())
    }
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use crate::vm::VMA_MANAGER;
    use ostd::prelude::ktest;

    #[ktest]
    fn test_map_and_unmap_region() {
        let vma_manager = VmaManager::new();
        vma_manager
            .map_region(0x1000, 0x2000, PageFlags::RW)
            .unwrap();
        let regions = vma_manager.regions.lock();
        assert!(regions.contains_key(&0x1000));
        drop(regions); // Release lock before unmapping
        vma_manager.unmap_region(0x1000, 0x2000).unwrap();
        let regions = vma_manager.regions.lock();
        assert!(!regions.contains_key(&0x1000));
    }

    #[ktest]
    fn test_copy_user() {
        let vma_manager = Arc::new(VmaManager::new());
        vma_manager.activate();

        vma_manager
            .map_region(0x2000, 0x1000, PageFlags::RW)
            .unwrap();

        let data_to_write = b"Hello from user space test!";
        let mut data_read_back = [0u8; 27];

        vma_manager.copy_to_user(0x2000, data_to_write).unwrap();
        vma_manager
            .copy_from_user(0x2000, &mut data_read_back)
            .unwrap();

        assert_eq!(data_to_write, &data_read_back);

        vma_manager.unmap_region(0x2000, 0x1000).unwrap();
    }

    #[ktest]
    fn test_guard_page_blocks_access() {
        crate::vm::init();
        let vma_manager = VMA_MANAGER.get().unwrap().clone();
        vma_manager.activate();

        let guard_size = PAGE_SIZE;
        let stack_size = PAGE_SIZE * 4;
        let stack_start = 0x90000;

        vma_manager
            .map_stack(stack_start, stack_size, guard_size)
            .unwrap();

        // Guard page address is at the bottom of the stack region
        let guard_addr = stack_start;
        let data = b"should not write to guard";

        // Writing to the guard page should fail
        assert!(vma_manager.copy_to_user(guard_addr, data).is_err());

        // Reading from the guard page should fail
        let mut buf = [0u8; 4];
        assert!(vma_manager.copy_from_user(guard_addr, &mut buf).is_err());

        vma_manager
            .unmap_region(stack_start, stack_size + guard_size)
            .unwrap();
    }

    #[ktest]
    fn test_stack_usable_area_works() {
        crate::vm::init();
        let vma_manager = VMA_MANAGER.get().unwrap().clone();
        vma_manager.activate();

        let guard_size = PAGE_SIZE;
        let stack_size = PAGE_SIZE * 4;
        let stack_start = 0xa0000;

        vma_manager
            .map_stack(stack_start, stack_size, guard_size)
            .unwrap();

        // Usable stack starts after the guard page
        let usable_start = stack_start + guard_size;
        let data = b"stack data works!";
        let mut buf = [0u8; 17];

        vma_manager.copy_to_user(usable_start, data).unwrap();
        vma_manager.copy_from_user(usable_start, &mut buf).unwrap();
        assert_eq!(data, &buf);

        // Also test the top of the stack area
        let top_addr = stack_start + guard_size + stack_size - PAGE_SIZE;
        let top_data = b"top of stack!";
        vma_manager.copy_to_user(top_addr, top_data).unwrap();
        let mut top_buf = [0u8; 13];
        vma_manager.copy_from_user(top_addr, &mut top_buf).unwrap();
        assert_eq!(top_data, &top_buf);

        vma_manager
            .unmap_region(stack_start, stack_size + guard_size)
            .unwrap();
    }
}
