use super::flags::*;
use super::index::*;
use super::utils::*;
use crate::mm::address::{PhysAddr, VirtAddr};
use crate::mm::paging::{MapError, MapFlags, PageTable, UnmapError};

pub struct ArchPageTable {
    pml4_phys: PhysAddr,
    is_owned: bool,
}

// SAFETY: PhysAddr is Send and Sync, and we perform thread-safe memory table accesses.
unsafe impl Send for ArchPageTable {}
unsafe impl Sync for ArchPageTable {}

fn free_table_recursive(paddr: PhysAddr, level: usize, hhdm: u64) {
    if level == 1 {
        crate::mm::PMM.free_page(paddr);
        return;
    }

    let table = paddr.as_ptr::<u64>(hhdm);
    let limit = if level == 4 { 256 } else { 512 };

    for i in 0..limit {
        let entry = unsafe { *table.add(i) };
        if (entry & PAGE_PRESENT) != 0 {
            // Do not recurse into huge pages (1GB at level 3, 2MB at level 2)
            if level > 1 && (entry & PAGE_HUGE) != 0 {
                continue;
            }
            let child_phys = PhysAddr(entry & 0x000F_FFFF_FFFF_F000);
            free_table_recursive(child_phys, level - 1, hhdm);
        }
    }

    crate::mm::PMM.free_page(paddr);
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
    fn new() -> Result<Self, MapError> {
        // SAFETY: Toggling EFER.NXE is valid on x86_64 architecture.
        unsafe {
            enable_nxe();
        }

        // Allocate a page for the PML4
        let pml4_phys = crate::mm::PMM
            .alloc_page()
            .ok_or(MapError::FrameAllocationFailed)?;
        let hhdm = hhdm_offset();
        let new_pml4 = pml4_phys.as_ptr::<u64>(hhdm);

        // Zero out the lower half (entries 0..256) and copy the higher half (entries 256..512)
        // to share kernel-space mappings.
        unsafe {
            core::ptr::write_bytes(new_pml4, 0, 512);

            let active_phys = active_cr3();
            let active_pml4 = active_phys.as_ptr::<u64>(hhdm);
            core::ptr::copy_nonoverlapping(active_pml4.add(256), new_pml4.add(256), 256);
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

    fn map(&mut self, page: VirtAddr, frame: PhysAddr, flags: MapFlags) -> Result<(), MapError> {
        if !page.is_aligned(4096) || !frame.is_aligned(4096) {
            return Err(MapError::InvalidAddress);
        }

        let hhdm = hhdm_offset();
        let l4_idx = pml4_index(page);
        let l3_idx = pdpt_index(page);
        let l2_idx = pd_index(page);
        let l1_idx = pt_index(page);

        // Walk levels from PML4 down to PT
        let pml4 = self.pml4_phys.as_ptr::<u64>(hhdm);
        let pml4_entry = unsafe { *pml4.add(l4_idx) };
        let pdpt_phys = if (pml4_entry & PAGE_PRESENT) == 0 {
            let new_frame = crate::mm::PMM
                .alloc_page()
                .ok_or(MapError::FrameAllocationFailed)?;
            let ptr = new_frame.as_ptr::<u8>(hhdm);
            unsafe {
                core::ptr::write_bytes(ptr, 0, 4096);
            }
            let new_entry = new_frame.as_u64() | PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER;
            unsafe {
                *pml4.add(l4_idx) = new_entry;
            }
            new_frame
        } else {
            PhysAddr(pml4_entry & 0x000F_FFFF_FFFF_F000)
        };

        let pdpt = pdpt_phys.as_ptr::<u64>(hhdm);
        let pdpt_entry = unsafe { *pdpt.add(l3_idx) };
        let pd_phys = if (pdpt_entry & PAGE_PRESENT) == 0 {
            let new_frame = crate::mm::PMM
                .alloc_page()
                .ok_or(MapError::FrameAllocationFailed)?;
            let ptr = new_frame.as_ptr::<u8>(hhdm);
            unsafe {
                core::ptr::write_bytes(ptr, 0, 4096);
            }
            let new_entry = new_frame.as_u64() | PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER;
            unsafe {
                *pdpt.add(l3_idx) = new_entry;
            }
            new_frame
        } else {
            PhysAddr(pdpt_entry & 0x000F_FFFF_FFFF_F000)
        };

        let pd = pd_phys.as_ptr::<u64>(hhdm);
        let pd_entry = unsafe { *pd.add(l2_idx) };
        let pt_phys = if (pd_entry & PAGE_PRESENT) == 0 {
            let new_frame = crate::mm::PMM
                .alloc_page()
                .ok_or(MapError::FrameAllocationFailed)?;
            let ptr = new_frame.as_ptr::<u8>(hhdm);
            unsafe {
                core::ptr::write_bytes(ptr, 0, 4096);
            }
            let new_entry = new_frame.as_u64() | PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER;
            unsafe {
                *pd.add(l2_idx) = new_entry;
            }
            new_frame
        } else {
            PhysAddr(pd_entry & 0x000F_FFFF_FFFF_F000)
        };

        let pt = pt_phys.as_ptr::<u64>(hhdm);
        let pt_entry = unsafe { *pt.add(l1_idx) };
        if (pt_entry & PAGE_PRESENT) != 0 {
            return Err(MapError::AlreadyMapped);
        }

        let entry_flags = translate_flags(flags);
        let pt_entry_val = frame.as_u64() | entry_flags;
        unsafe {
            *pt.add(l1_idx) = pt_entry_val;
            // Flush TLB for this virtual address
            core::arch::asm!("invlpg [{}]", in(reg) page.as_u64(), options(nostack, preserves_flags));
        }

        Ok(())
    }

    fn unmap(&mut self, page: VirtAddr) -> Result<PhysAddr, UnmapError> {
        if !page.is_aligned(4096) {
            return Err(UnmapError::InvalidAddress);
        }

        let hhdm = hhdm_offset();
        let l4_idx = pml4_index(page);
        let l3_idx = pdpt_index(page);
        let l2_idx = pd_index(page);
        let l1_idx = pt_index(page);

        let pml4 = self.pml4_phys.as_ptr::<u64>(hhdm);
        let pml4_entry = unsafe { *pml4.add(l4_idx) };
        if (pml4_entry & PAGE_PRESENT) == 0 {
            return Err(UnmapError::NotMapped);
        }

        let pdpt_phys = PhysAddr(pml4_entry & 0x000F_FFFF_FFFF_F000);
        let pdpt = pdpt_phys.as_ptr::<u64>(hhdm);
        let pdpt_entry = unsafe { *pdpt.add(l3_idx) };
        if (pdpt_entry & PAGE_PRESENT) == 0 {
            return Err(UnmapError::NotMapped);
        }

        let pd_phys = PhysAddr(pdpt_entry & 0x000F_FFFF_FFFF_F000);
        let pd = pd_phys.as_ptr::<u64>(hhdm);
        let pd_entry = unsafe { *pd.add(l2_idx) };
        if (pd_entry & PAGE_PRESENT) == 0 {
            return Err(UnmapError::NotMapped);
        }

        let pt_phys = PhysAddr(pd_entry & 0x000F_FFFF_FFFF_F000);
        let pt = pt_phys.as_ptr::<u64>(hhdm);
        let pt_entry = unsafe { *pt.add(l1_idx) };
        if (pt_entry & PAGE_PRESENT) == 0 {
            return Err(UnmapError::NotMapped);
        }

        let frame_phys = PhysAddr(pt_entry & 0x000F_FFFF_FFFF_F000);

        unsafe {
            *pt.add(l1_idx) = 0;
            // Invalidate TLB for this virtual address
            core::arch::asm!("invlpg [{}]", in(reg) page.as_u64(), options(nostack, preserves_flags));
        }

        Ok(frame_phys)
    }

    fn translate(&self, virt: VirtAddr) -> Option<PhysAddr> {
        let hhdm = hhdm_offset();
        let l4_idx = pml4_index(virt);
        let l3_idx = pdpt_index(virt);
        let l2_idx = pd_index(virt);
        let l1_idx = pt_index(virt);

        let pml4 = self.pml4_phys.as_ptr::<u64>(hhdm);
        let pml4_entry = unsafe { *pml4.add(l4_idx) };
        if (pml4_entry & PAGE_PRESENT) == 0 {
            return None;
        }

        let pdpt_phys = PhysAddr(pml4_entry & 0x000F_FFFF_FFFF_F000);
        let pdpt = pdpt_phys.as_ptr::<u64>(hhdm);
        let pdpt_entry = unsafe { *pdpt.add(l3_idx) };
        if (pdpt_entry & PAGE_PRESENT) == 0 {
            return None;
        }

        // Support 1GB huge pages
        if (pdpt_entry & PAGE_HUGE) != 0 {
            let offset = virt.as_u64() & 0x3FFF_FFFF;
            return Some(PhysAddr((pdpt_entry & 0x000F_FFFF_C000_0000) + offset));
        }

        let pd_phys = PhysAddr(pdpt_entry & 0x000F_FFFF_FFFF_F000);
        let pd = pd_phys.as_ptr::<u64>(hhdm);
        let pd_entry = unsafe { *pd.add(l2_idx) };
        if (pd_entry & PAGE_PRESENT) == 0 {
            return None;
        }

        // Support 2MB huge pages
        if (pd_entry & PAGE_HUGE) != 0 {
            let offset = virt.as_u64() & 0x1F_FFFF;
            return Some(PhysAddr((pd_entry & 0x000F_FFFF_FFE0_0000) + offset));
        }

        let pt_phys = PhysAddr(pd_entry & 0x000F_FFFF_FFFF_F000);
        let pt = pt_phys.as_ptr::<u64>(hhdm);
        let pt_entry = unsafe { *pt.add(l1_idx) };
        if (pt_entry & PAGE_PRESENT) == 0 {
            return None;
        }

        let offset = virt.as_u64() & 0xFFF;
        Some(PhysAddr((pt_entry & 0x000F_FFFF_FFFF_F000) + offset))
    }

    unsafe fn activate(&self) {
        // SAFETY: Switch CR3 register to reload the active page tables.
        unsafe {
            core::arch::asm!("mov cr3, {}", in(reg) self.pml4_phys.as_u64());
        }
    }
}
