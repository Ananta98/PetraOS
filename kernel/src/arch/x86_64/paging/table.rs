use super::flags::enable_nxe;
use super::frame::KernelFrameAllocator;
use crate::arch::{active_address_space_root, set_address_space_root};
use crate::mm::hhdm_offset;
use crate::mm::pmm::PMM;
use crate::mm::vmm::PageTable;
use x86_64::structures::paging::mapper::{
    FlagUpdateError, MapToError, Mapper, Translate, TranslateResult, UnmapError,
};
use x86_64::structures::paging::{
    OffsetPageTable, Page, PageTable as X86PageTable, PageTableFlags, PhysFrame, Size4KiB,
};
use x86_64::{PhysAddr, VirtAddr};

/// Architecture-specific x86_64 Page Table Implementation utilizing `OffsetPageTable`.
pub struct ArchPageTable {
    pml4_phys: PhysAddr,
    is_owned: bool,
}

unsafe impl Send for ArchPageTable {}
unsafe impl Sync for ArchPageTable {}

impl ArchPageTable {
    /// Creates an `OffsetPageTable` mapper instance targeting this PML4 root directory.
    unsafe fn get_offset_page_table(&self) -> OffsetPageTable<'static> {
        let hhdm = hhdm_offset();
        let pml4_ptr = (self.pml4_phys.as_u64() + hhdm) as *mut X86PageTable;
        // SAFETY: pml4_ptr is a valid PML4 pointer mapped at HHDM offset.
        unsafe { OffsetPageTable::new(&mut *pml4_ptr, VirtAddr::new(hhdm)) }
    }
}

fn free_table_recursive(paddr: PhysAddr, level: usize, hhdm: u64) {
    if level == 1 {
        PMM.free_page(paddr);
        return;
    }

    let table_ptr = (paddr.as_u64() + hhdm) as *mut X86PageTable;
    let table = unsafe { &*table_ptr };
    let limit = if level == 4 { 256 } else { 512 };

    for i in 0..limit {
        let entry = &table[i];
        if entry.flags().contains(PageTableFlags::PRESENT) {
            let child_phys = entry.addr();
            if level > 1 && entry.flags().contains(PageTableFlags::HUGE_PAGE) {
                PMM.free_page(child_phys);
                continue;
            }
            free_table_recursive(child_phys, level - 1, hhdm);
        }
    }

    PMM.free_page(paddr);
}

impl Drop for ArchPageTable {
    fn drop(&mut self) {
        if self.is_owned {
            let hhdm = hhdm_offset();
            free_table_recursive(self.pml4_phys, 4, hhdm);
        }
    }
}

impl PageTable for ArchPageTable {
    fn new() -> Result<Self, MapToError<Size4KiB>> {
        // SAFETY: Toggling EFER.NXE is valid on x86_64 architecture.
        unsafe {
            enable_nxe();
        }

        // Allocate a page for the PML4
        let pml4_phys = PMM.alloc_page().ok_or(MapToError::FrameAllocationFailed)?;
        let hhdm = hhdm_offset();
        let new_pml4_ptr = (pml4_phys.as_u64() + hhdm) as *mut X86PageTable;

        // Zero all PML4 entries, then copy the higher-half kernel entries (256..512)
        unsafe {
            let new_pml4 = &mut *new_pml4_ptr;
            new_pml4.zero();

            let active_phys = active_address_space_root();
            let active_pml4 = &*((active_phys + hhdm) as *const X86PageTable);
            for i in 256..512 {
                new_pml4[i] = active_pml4[i].clone();
            }
        }

        Ok(Self {
            pml4_phys,
            is_owned: true,
        })
    }

    unsafe fn from_root(root: PhysAddr) -> Self {
        // SAFETY: Toggling EFER.NXE is valid on x86_64 architecture.
        unsafe {
            enable_nxe();
        }
        Self {
            pml4_phys: root,
            is_owned: false,
        }
    }

    fn root(&self) -> PhysAddr {
        self.pml4_phys
    }

    fn map(
        &mut self,
        page: VirtAddr,
        frame: PhysAddr,
        flags: PageTableFlags,
    ) -> Result<(), MapToError<Size4KiB>> {
        if !page.is_aligned(4096u64) || !frame.is_aligned(4096u64) {
            return Err(MapToError::FrameAllocationFailed);
        }

        let target_page = Page::<Size4KiB>::from_start_address(page)
            .map_err(|_| MapToError::FrameAllocationFailed)?;
        let target_frame = PhysFrame::<Size4KiB>::from_start_address(frame)
            .map_err(|_| MapToError::FrameAllocationFailed)?;

        let mut mapper = unsafe { self.get_offset_page_table() };
        let mut frame_allocator = KernelFrameAllocator;

        // All intermediate directory levels (PML4, PDPT, PD) are created with full permissions
        // so that individual page permissions are governed by the leaf PTE.
        let parent_table_flags = PageTableFlags::PRESENT
            | PageTableFlags::WRITABLE
            | PageTableFlags::USER_ACCESSIBLE;

        // SAFETY: Updating page tables using standard OffsetPageTable mapper.
        unsafe {
            mapper
                .map_to_with_table_flags(
                    target_page,
                    target_frame,
                    flags,
                    parent_table_flags,
                    &mut frame_allocator,
                )?
                .flush();
        }

        Ok(())
    }

    fn map_range(
        &mut self,
        page: VirtAddr,
        frame: PhysAddr,
        size: usize,
        flags: PageTableFlags,
    ) -> Result<(), MapToError<Size4KiB>> {
        let count = (size + 4095) / 4096;
        for i in 0..count {
            self.map(
                page + (i as u64 * 4096),
                frame + (i as u64 * 4096),
                flags,
            )?;
        }
        Ok(())
    }

    fn unmap(&mut self, page: VirtAddr) -> Result<PhysAddr, UnmapError> {
        if !page.is_aligned(4096u64) {
            return Err(UnmapError::InvalidFrameAddress(PhysAddr::zero()));
        }

        let target_page = Page::<Size4KiB>::from_start_address(page)
            .map_err(|_| UnmapError::InvalidFrameAddress(PhysAddr::zero()))?;

        let mut mapper = unsafe { self.get_offset_page_table() };
        let (frame, flush) = mapper.unmap(target_page)?;

        flush.flush();
        Ok(frame.start_address())
    }

    fn unmap_range(&mut self, page: VirtAddr, size: usize) -> Result<(), UnmapError> {
        let count = (size + 4095) / 4096;
        for i in 0..count {
            self.unmap(page + (i as u64 * 4096))?;
        }
        Ok(())
    }

    fn remap(&mut self, page: VirtAddr, flags: PageTableFlags) -> Result<(), FlagUpdateError> {
        if !page.is_aligned(4096u64) {
            return Err(FlagUpdateError::PageNotMapped);
        }

        let target_page = Page::<Size4KiB>::from_start_address(page)
            .map_err(|_| FlagUpdateError::PageNotMapped)?;

        let mut mapper = unsafe { self.get_offset_page_table() };
        // SAFETY: Updating page table flags with valid flags on mapped page.
        let flush = unsafe { mapper.update_flags(target_page, flags)? };

        flush.flush();
        Ok(())
    }

    fn remap_range(
        &mut self,
        page: VirtAddr,
        size: usize,
        flags: PageTableFlags,
    ) -> Result<(), FlagUpdateError> {
        let count = (size + 4095) / 4096;
        for i in 0..count {
            self.remap(page + (i as u64 * 4096), flags)?;
        }
        Ok(())
    }

    fn translate(&self, virt: VirtAddr) -> Option<PhysAddr> {
        let mapper = unsafe { self.get_offset_page_table() };
        match mapper.translate(virt) {
            TranslateResult::Mapped { frame, offset, .. } => {
                Some(frame.start_address() + offset)
            }
            _ => None,
        }
    }

    fn get_entry(&self, virt: VirtAddr) -> Option<(PhysAddr, PageTableFlags)> {
        let mapper = unsafe { self.get_offset_page_table() };
        match mapper.translate(virt) {
            TranslateResult::Mapped {
                frame,
                offset,
                flags,
            } => Some((frame.start_address() + offset, flags)),
            _ => None,
        }
    }

    unsafe fn activate(&self) {
        unsafe {
            set_address_space_root(self.pml4_phys.as_u64());
        }
    }
}
