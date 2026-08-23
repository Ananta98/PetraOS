//! Architecture-Specific x86_64 Page Table Implementation.
//!
//! Supports both 4-level (PML4) and 5-level (PML5 / LA57) hardware page tables
//! with manual table traversal, demand page mapping, and TLB invalidation.

use super::flush;
use super::helpers::enable_nxe;
use super::active_paging_levels;
use crate::arch::{active_address_space_root, set_address_space_root};
use crate::mm::hhdm_offset;
use crate::mm::pmm::PMM;
use crate::mm::vmm::paging::entry::PageTableEntry;
use crate::mm::vmm::paging::{PageTable, PagingError};
use crate::mm::{PageTableFlags, PhysAddr, VirtAddr};

/// Architecture-specific x86_64 page table implementation.
pub struct ArchPageTable {
    root_phys: PhysAddr,
    is_owned: bool,
    levels: u8,
}

unsafe impl Send for ArchPageTable {}
unsafe impl Sync for ArchPageTable {}

impl ArchPageTable {
    /// Returns the number of paging levels configured (4 or 5).
    #[inline(always)]
    pub fn levels(&self) -> u8 {
        self.levels
    }

    /// Access a page table at physical address `table_phys` as a slice of 512 entries.
    #[inline(always)]
    unsafe fn get_table_mut(table_phys: PhysAddr) -> &'static mut [PageTableEntry; 512] {
        let hhdm = hhdm_offset();
        let ptr = (table_phys.as_u64() + hhdm) as *mut PageTableEntry;
        // SAFETY: Pointer is valid at HHDM offset and aligned to 4096 bytes.
        unsafe { &mut *(ptr as *mut [PageTableEntry; 512]) }
    }

    /// Traverses page tables from the root down to the Level 1 (PT) entry for `virt`.
    ///
    /// If `allocate` is true, intermediate page tables will be allocated from the PMM as needed.
    fn walk_to_pte_mut(
        &mut self,
        virt: VirtAddr,
        allocate: bool,
    ) -> Result<Option<&'static mut PageTableEntry>, PagingError> {
        let mut curr_phys = self.root_phys;

        // Level 5 (PML5) if 5-level paging is active
        if self.levels >= 5 {
            let pml5 = unsafe { Self::get_table_mut(curr_phys) };
            let entry = &mut pml5[virt.pml5_index()];
            if !entry.is_present() {
                if !allocate {
                    return Ok(None);
                }
                let new_frame = PMM.alloc_page().ok_or(PagingError::FrameAllocationFailed)?;
                let hhdm = hhdm_offset();
                unsafe {
                    core::ptr::write_bytes((new_frame.as_u64() + hhdm) as *mut u8, 0, 4096);
                }
                entry.set(
                    new_frame,
                    PageTableFlags::PRESENT
                        | PageTableFlags::WRITABLE
                        | PageTableFlags::USER_ACCESSIBLE,
                );
            }
            curr_phys = entry.addr();
        }

        // Level 4 (PML4)
        let pml4 = unsafe { Self::get_table_mut(curr_phys) };
        let pml4_entry = &mut pml4[virt.pml4_index()];
        if !pml4_entry.is_present() {
            if !allocate {
                return Ok(None);
            }
            let new_frame = PMM.alloc_page().ok_or(PagingError::FrameAllocationFailed)?;
            let hhdm = hhdm_offset();
            unsafe {
                core::ptr::write_bytes((new_frame.as_u64() + hhdm) as *mut u8, 0, 4096);
            }
            pml4_entry.set(
                new_frame,
                PageTableFlags::PRESENT
                    | PageTableFlags::WRITABLE
                    | PageTableFlags::USER_ACCESSIBLE,
            );
        }
        curr_phys = pml4_entry.addr();

        // Level 3 (PDPT)
        let pdpt = unsafe { Self::get_table_mut(curr_phys) };
        let pdpt_entry = &mut pdpt[virt.pdpt_index()];
        if pdpt_entry.is_huge() {
            return Err(PagingError::HugePageConflict);
        }
        if !pdpt_entry.is_present() {
            if !allocate {
                return Ok(None);
            }
            let new_frame = PMM.alloc_page().ok_or(PagingError::FrameAllocationFailed)?;
            let hhdm = hhdm_offset();
            unsafe {
                core::ptr::write_bytes((new_frame.as_u64() + hhdm) as *mut u8, 0, 4096);
            }
            pdpt_entry.set(
                new_frame,
                PageTableFlags::PRESENT
                    | PageTableFlags::WRITABLE
                    | PageTableFlags::USER_ACCESSIBLE,
            );
        }
        curr_phys = pdpt_entry.addr();

        // Level 2 (PD)
        let pd = unsafe { Self::get_table_mut(curr_phys) };
        let pd_entry = &mut pd[virt.pd_index()];
        if pd_entry.is_huge() {
            return Err(PagingError::HugePageConflict);
        }
        if !pd_entry.is_present() {
            if !allocate {
                return Ok(None);
            }
            let new_frame = PMM.alloc_page().ok_or(PagingError::FrameAllocationFailed)?;
            let hhdm = hhdm_offset();
            unsafe {
                core::ptr::write_bytes((new_frame.as_u64() + hhdm) as *mut u8, 0, 4096);
            }
            pd_entry.set(
                new_frame,
                PageTableFlags::PRESENT
                    | PageTableFlags::WRITABLE
                    | PageTableFlags::USER_ACCESSIBLE,
            );
        }
        curr_phys = pd_entry.addr();

        // Level 1 (PT)
        let pt = unsafe { Self::get_table_mut(curr_phys) };
        Ok(Some(&mut pt[virt.pt_index()]))
    }
}

fn free_table_recursive(paddr: PhysAddr, level: u8, hhdm: u64) {
    if level <= 1 {
        PMM.free_page(paddr);
        return;
    }

    let table_ptr = (paddr.as_u64() + hhdm) as *mut PageTableEntry;
    let table = unsafe { &*core::ptr::slice_from_raw_parts(table_ptr, 512) };

    for entry in table.iter() {
        if entry.is_present() {
            let child_phys = entry.addr();
            if entry.is_huge() {
                PMM.free_page(child_phys);
            } else {
                free_table_recursive(child_phys, level - 1, hhdm);
            }
        }
    }

    PMM.free_page(paddr);
}

impl Drop for ArchPageTable {
    fn drop(&mut self) {
        if self.is_owned {
            let hhdm = hhdm_offset();
            let table_ptr = (self.root_phys.as_u64() + hhdm) as *mut PageTableEntry;
            let table = unsafe { &*core::ptr::slice_from_raw_parts(table_ptr, 512) };

            // Only recurse into user-space entries (0..256) of the root directory.
            // Higher-half entries (256..512) belong to shared kernel space and must not be freed.
            for i in 0..256 {
                let entry = table[i];
                if entry.is_present() {
                    let child_phys = entry.addr();
                    if entry.is_huge() {
                        PMM.free_page(child_phys);
                    } else {
                        free_table_recursive(child_phys, self.levels - 1, hhdm);
                    }
                }
            }

            PMM.free_page(self.root_phys);
        }
    }
}

impl PageTable for ArchPageTable {
    fn new() -> Result<Self, PagingError> {
        // SAFETY: Toggling EFER.NXE is valid on x86_64 architecture.
        unsafe {
            enable_nxe();
        }

        let levels = active_paging_levels();

        // Allocate a page for the root directory (PML4 or PML5)
        let root_phys = PMM.alloc_page().ok_or(PagingError::FrameAllocationFailed)?;
        let hhdm = hhdm_offset();
        let new_root_ptr = (root_phys.as_u64() + hhdm) as *mut PageTableEntry;

        // Zero all 512 entries, then copy higher-half kernel entries (256..512)
        unsafe {
            core::ptr::write_bytes(new_root_ptr as *mut u8, 0, 4096);
            let new_root = &mut *(new_root_ptr as *mut [PageTableEntry; 512]);

            let active_phys = active_address_space_root();
            let active_root = &*((active_phys + hhdm) as *const [PageTableEntry; 512]);

            for i in 256..512 {
                new_root[i] = active_root[i];
            }
        }

        Ok(Self {
            root_phys,
            is_owned: true,
            levels,
        })
    }

    unsafe fn from_root(root: PhysAddr) -> Self {
        // SAFETY: Toggling EFER.NXE is valid on x86_64 architecture.
        unsafe {
            enable_nxe();
        }
        Self {
            root_phys: root,
            is_owned: false,
            levels: active_paging_levels(),
        }
    }

    fn root(&self) -> PhysAddr {
        self.root_phys
    }

    fn map(
        &mut self,
        page: VirtAddr,
        frame: PhysAddr,
        flags: PageTableFlags,
    ) -> Result<(), PagingError> {
        if !page.is_aligned(4096) || !frame.is_aligned(4096) {
            return Err(PagingError::InvalidAddress);
        }

        let pte = self
            .walk_to_pte_mut(page, true)?
            .ok_or(PagingError::FrameAllocationFailed)?;

        if pte.is_present() {
            return Err(PagingError::AlreadyMapped);
        }

        pte.set(frame, flags);
        self.flush_tlb(page);
        Ok(())
    }

    fn map_range(
        &mut self,
        page: VirtAddr,
        frame: PhysAddr,
        size: usize,
        flags: PageTableFlags,
    ) -> Result<(), PagingError> {
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

    fn unmap(&mut self, page: VirtAddr) -> Result<PhysAddr, PagingError> {
        if !page.is_aligned(4096) {
            return Err(PagingError::InvalidAddress);
        }

        let pte = self
            .walk_to_pte_mut(page, false)?
            .ok_or(PagingError::NotMapped)?;

        if !pte.is_present() {
            return Err(PagingError::NotMapped);
        }

        let frame_addr = pte.addr();
        pte.clear();
        self.flush_tlb(page);
        Ok(frame_addr)
    }

    fn unmap_range(&mut self, page: VirtAddr, size: usize) -> Result<(), PagingError> {
        let count = (size + 4095) / 4096;
        for i in 0..count {
            self.unmap(page + (i as u64 * 4096))?;
        }
        Ok(())
    }

    fn remap(&mut self, page: VirtAddr, flags: PageTableFlags) -> Result<(), PagingError> {
        if !page.is_aligned(4096) {
            return Err(PagingError::InvalidAddress);
        }

        let pte = self
            .walk_to_pte_mut(page, false)?
            .ok_or(PagingError::NotMapped)?;

        if !pte.is_present() {
            return Err(PagingError::NotMapped);
        }

        pte.set_flags(flags);
        self.flush_tlb(page);
        Ok(())
    }

    fn remap_range(
        &mut self,
        page: VirtAddr,
        size: usize,
        flags: PageTableFlags,
    ) -> Result<(), PagingError> {
        let count = (size + 4095) / 4096;
        for i in 0..count {
            self.remap(page + (i as u64 * 4096), flags)?;
        }
        Ok(())
    }

    fn translate(&self, virt: VirtAddr) -> Option<PhysAddr> {
        let mut curr_phys = self.root_phys;

        // Level 5 (PML5)
        if self.levels >= 5 {
            let pml5 = unsafe { Self::get_table_mut(curr_phys) };
            let entry = pml5[virt.pml5_index()];
            if !entry.is_present() {
                return None;
            }
            curr_phys = entry.addr();
        }

        // Level 4 (PML4)
        let pml4 = unsafe { Self::get_table_mut(curr_phys) };
        let pml4_entry = pml4[virt.pml4_index()];
        if !pml4_entry.is_present() {
            return None;
        }
        curr_phys = pml4_entry.addr();

        // Level 3 (PDPT)
        let pdpt = unsafe { Self::get_table_mut(curr_phys) };
        let pdpt_entry = pdpt[virt.pdpt_index()];
        if !pdpt_entry.is_present() {
            return None;
        }
        if pdpt_entry.is_huge() {
            // 1 GiB page
            return Some(pdpt_entry.addr() + (virt.as_u64() & 0x3FFF_FFFF));
        }
        curr_phys = pdpt_entry.addr();

        // Level 2 (PD)
        let pd = unsafe { Self::get_table_mut(curr_phys) };
        let pd_entry = pd[virt.pd_index()];
        if !pd_entry.is_present() {
            return None;
        }
        if pd_entry.is_huge() {
            // 2 MiB page
            return Some(pd_entry.addr() + (virt.as_u64() & 0x1F_FFFF));
        }
        curr_phys = pd_entry.addr();

        // Level 1 (PT)
        let pt = unsafe { Self::get_table_mut(curr_phys) };
        let pt_entry = pt[virt.pt_index()];
        if !pt_entry.is_present() {
            return None;
        }

        Some(pt_entry.addr() + virt.page_offset())
    }

    fn get_entry(&self, virt: VirtAddr) -> Option<(PhysAddr, PageTableFlags)> {
        let mut curr_phys = self.root_phys;

        // Level 5 (PML5)
        if self.levels >= 5 {
            let pml5 = unsafe { Self::get_table_mut(curr_phys) };
            let entry = pml5[virt.pml5_index()];
            if !entry.is_present() {
                return None;
            }
            curr_phys = entry.addr();
        }

        // Level 4 (PML4)
        let pml4 = unsafe { Self::get_table_mut(curr_phys) };
        let pml4_entry = pml4[virt.pml4_index()];
        if !pml4_entry.is_present() {
            return None;
        }
        curr_phys = pml4_entry.addr();

        // Level 3 (PDPT)
        let pdpt = unsafe { Self::get_table_mut(curr_phys) };
        let pdpt_entry = pdpt[virt.pdpt_index()];
        if !pdpt_entry.is_present() {
            return None;
        }
        if pdpt_entry.is_huge() {
            return Some((
                pdpt_entry.addr() + (virt.as_u64() & 0x3FFF_FFFF),
                pdpt_entry.flags(),
            ));
        }
        curr_phys = pdpt_entry.addr();

        // Level 2 (PD)
        let pd = unsafe { Self::get_table_mut(curr_phys) };
        let pd_entry = pd[virt.pd_index()];
        if !pd_entry.is_present() {
            return None;
        }
        if pd_entry.is_huge() {
            return Some((
                pd_entry.addr() + (virt.as_u64() & 0x1F_FFFF),
                pd_entry.flags(),
            ));
        }
        curr_phys = pd_entry.addr();

        // Level 1 (PT)
        let pt = unsafe { Self::get_table_mut(curr_phys) };
        let pt_entry = pt[virt.pt_index()];
        if !pt_entry.is_present() {
            return None;
        }

        Some((pt_entry.addr() + virt.page_offset(), pt_entry.flags()))
    }

    fn flush_tlb(&self, page: VirtAddr) {
        flush::invlpg(page);
    }

    fn flush_tlb_all(&self) {
        flush::flush_all();
    }

    unsafe fn activate(&self) {
        unsafe {
            set_address_space_root(self.root_phys.as_u64());
        }
    }
}
