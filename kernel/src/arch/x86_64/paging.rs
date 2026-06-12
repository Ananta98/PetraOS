use crate::mm::address::{PhysAddr, VirtAddr};
use crate::mm::paging::{MapError, MapFlags, PageTable, UnmapError};
use core::sync::atomic::AtomicU64;

pub struct X86_64PageTable {
    pml4_phys: PhysAddr,
    is_owned: bool,
}

// SAFETY: PhysAddr is Send and Sync, and we perform thread-safe memory table accesses.
unsafe impl Send for X86_64PageTable {}
unsafe impl Sync for X86_64PageTable {}

fn hhdm_offset() -> u64 {
    static OFFSET: AtomicU64 = AtomicU64::new(u64::MAX);
    let val = OFFSET.load(core::sync::atomic::Ordering::Relaxed);
    if val != u64::MAX {
        return val;
    }
    let offset = crate::limine::HHDM_REQUEST
        .get_response()
        .expect("Paging: Limine HHDM response is missing")
        .offset();
    OFFSET.store(offset, core::sync::atomic::Ordering::Relaxed);
    offset
}

pub unsafe fn active_cr3() -> PhysAddr {
    let cr3: u64;
    // SAFETY: Reading CR3 register is required to find the current PML4 physical address.
    unsafe {
        core::arch::asm!("mov {}, cr3", out(reg) cr3);
    }
    PhysAddr(cr3 & !0xFFF)
}

/// Enable the No-Execute (NXE) bit in the Extended Feature Enable Register (EFER) MSR.
/// This allows the MMU to enforce execution prevention (NX bit).
pub unsafe fn enable_nxe() {
    let msr = 0xC0000080u32;
    let mut low: u32;
    let mut high: u32;
    // SAFETY: rdmsr is used to fetch the EFER register.
    unsafe {
        core::arch::asm!(
            "rdmsr",
            in("ecx") msr,
            out("eax") low,
            out("edx") high,
        );
    }
    low |= 1 << 11; // NXE bit
    // SAFETY: wrmsr is used to write updated flags back to EFER.
    unsafe {
        core::arch::asm!(
            "wrmsr",
            in("ecx") msr,
            in("eax") low,
            in("edx") high,
        );
    }
}

fn free_table_recursive(paddr: PhysAddr, level: usize, hhdm: u64) {
    if level == 1 {
        crate::mm::PMM.free_page(paddr);
        return;
    }

    let table = paddr.as_ptr::<u64>(hhdm);
    let limit = if level == 4 { 256 } else { 512 };

    for i in 0..limit {
        let entry = unsafe { *table.add(i) };
        if (entry & 1) != 0 {
            // Do not recurse into huge pages (1GB at level 3, 2MB at level 2)
            if level > 1 && (entry & (1 << 7)) != 0 {
                continue;
            }
            let child_phys = PhysAddr(entry & 0x000F_FFFF_FFFF_F000);
            free_table_recursive(child_phys, level - 1, hhdm);
        }
    }

    crate::mm::PMM.free_page(paddr);
}

impl Drop for X86_64PageTable {
    fn drop(&mut self) {
        if self.is_owned {
            let hhdm = hhdm_offset();
            free_table_recursive(self.pml4_phys, 4, hhdm);
        }
    }
}

fn pml4_index(virt: VirtAddr) -> usize {
    ((virt.as_u64() >> 39) & 0x1FF) as usize
}
fn pdpt_index(virt: VirtAddr) -> usize {
    ((virt.as_u64() >> 30) & 0x1FF) as usize
}
fn pd_index(virt: VirtAddr) -> usize {
    ((virt.as_u64() >> 21) & 0x1FF) as usize
}
fn pt_index(virt: VirtAddr) -> usize {
    ((virt.as_u64() >> 12) & 0x1FF) as usize
}

fn translate_flags(flags: MapFlags) -> u64 {
    let mut entry_flags = 1u64; // Present (bit 0) is always 1
    if flags.contains(MapFlags::WRITE) {
        entry_flags |= 1 << 1; // Writable
    }
    if flags.contains(MapFlags::USER) {
        entry_flags |= 1 << 2; // User-accessible
    }
    if flags.contains(MapFlags::NO_CACHE) {
        entry_flags |= 1 << 4; // Cache-disable
    }
    if !flags.contains(MapFlags::EXECUTE) {
        entry_flags |= 1 << 63; // No-Execute (NX)
    }
    entry_flags
}

impl PageTable for X86_64PageTable {
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
        let pdpt_phys = if (pml4_entry & 1) == 0 {
            let new_frame = crate::mm::PMM
                .alloc_page()
                .ok_or(MapError::FrameAllocationFailed)?;
            let ptr = new_frame.as_ptr::<u8>(hhdm);
            unsafe {
                core::ptr::write_bytes(ptr, 0, 4096);
            }
            let new_entry = new_frame.as_u64() | 1 | 2 | 4; // Present | Writable | User
            unsafe {
                *pml4.add(l4_idx) = new_entry;
            }
            new_frame
        } else {
            PhysAddr(pml4_entry & 0x000F_FFFF_FFFF_F000)
        };

        let pdpt = pdpt_phys.as_ptr::<u64>(hhdm);
        let pdpt_entry = unsafe { *pdpt.add(l3_idx) };
        let pd_phys = if (pdpt_entry & 1) == 0 {
            let new_frame = crate::mm::PMM
                .alloc_page()
                .ok_or(MapError::FrameAllocationFailed)?;
            let ptr = new_frame.as_ptr::<u8>(hhdm);
            unsafe {
                core::ptr::write_bytes(ptr, 0, 4096);
            }
            let new_entry = new_frame.as_u64() | 1 | 2 | 4; // Present | Writable | User
            unsafe {
                *pdpt.add(l3_idx) = new_entry;
            }
            new_frame
        } else {
            PhysAddr(pdpt_entry & 0x000F_FFFF_FFFF_F000)
        };

        let pd = pd_phys.as_ptr::<u64>(hhdm);
        let pd_entry = unsafe { *pd.add(l2_idx) };
        let pt_phys = if (pd_entry & 1) == 0 {
            let new_frame = crate::mm::PMM
                .alloc_page()
                .ok_or(MapError::FrameAllocationFailed)?;
            let ptr = new_frame.as_ptr::<u8>(hhdm);
            unsafe {
                core::ptr::write_bytes(ptr, 0, 4096);
            }
            let new_entry = new_frame.as_u64() | 1 | 2 | 4; // Present | Writable | User
            unsafe {
                *pd.add(l2_idx) = new_entry;
            }
            new_frame
        } else {
            PhysAddr(pd_entry & 0x000F_FFFF_FFFF_F000)
        };

        let pt = pt_phys.as_ptr::<u64>(hhdm);
        let pt_entry = unsafe { *pt.add(l1_idx) };
        if (pt_entry & 1) != 0 {
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
        if (pml4_entry & 1) == 0 {
            return Err(UnmapError::NotMapped);
        }

        let pdpt_phys = PhysAddr(pml4_entry & 0x000F_FFFF_FFFF_F000);
        let pdpt = pdpt_phys.as_ptr::<u64>(hhdm);
        let pdpt_entry = unsafe { *pdpt.add(l3_idx) };
        if (pdpt_entry & 1) == 0 {
            return Err(UnmapError::NotMapped);
        }

        let pd_phys = PhysAddr(pdpt_entry & 0x000F_FFFF_FFFF_F000);
        let pd = pd_phys.as_ptr::<u64>(hhdm);
        let pd_entry = unsafe { *pd.add(l2_idx) };
        if (pd_entry & 1) == 0 {
            return Err(UnmapError::NotMapped);
        }

        let pt_phys = PhysAddr(pd_entry & 0x000F_FFFF_FFFF_F000);
        let pt = pt_phys.as_ptr::<u64>(hhdm);
        let pt_entry = unsafe { *pt.add(l1_idx) };
        if (pt_entry & 1) == 0 {
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
        if (pml4_entry & 1) == 0 {
            return None;
        }

        let pdpt_phys = PhysAddr(pml4_entry & 0x000F_FFFF_FFFF_F000);
        let pdpt = pdpt_phys.as_ptr::<u64>(hhdm);
        let pdpt_entry = unsafe { *pdpt.add(l3_idx) };
        if (pdpt_entry & 1) == 0 {
            return None;
        }

        // Support 1GB huge pages
        if (pdpt_entry & (1 << 7)) != 0 {
            let offset = virt.as_u64() & 0x3FFF_FFFF;
            return Some(PhysAddr((pdpt_entry & 0x000F_FFFF_C000_0000) + offset));
        }

        let pd_phys = PhysAddr(pdpt_entry & 0x000F_FFFF_FFFF_F000);
        let pd = pd_phys.as_ptr::<u64>(hhdm);
        let pd_entry = unsafe { *pd.add(l2_idx) };
        if (pd_entry & 1) == 0 {
            return None;
        }

        // Support 2MB huge pages
        if (pd_entry & (1 << 7)) != 0 {
            let offset = virt.as_u64() & 0x1F_FFFF;
            return Some(PhysAddr((pd_entry & 0x000F_FFFF_FFE0_0000) + offset));
        }

        let pt_phys = PhysAddr(pd_entry & 0x000F_FFFF_FFFF_F000);
        let pt = pt_phys.as_ptr::<u64>(hhdm);
        let pt_entry = unsafe { *pt.add(l1_idx) };
        if (pt_entry & 1) == 0 {
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

/// Helper to ensure a physical memory range is mapped in the active page table.
pub fn ensure_mapped(phys_addr: u64, size: usize) {
    let hhdm = hhdm_offset();
    unsafe {
        let active_table_phys = active_cr3();
        let mut page_table = X86_64PageTable::from_root(active_table_phys);

        let start_page_phys = phys_addr & !4095;
        let end_page_phys = (phys_addr + size as u64 - 1) & !4095;

        let mut curr_phys = start_page_phys;
        while curr_phys <= end_page_phys {
            let curr_virt = curr_phys + hhdm;
            let _ = page_table.map(
                VirtAddr(curr_virt),
                PhysAddr(curr_phys),
                MapFlags::WRITE | MapFlags::EXECUTE,
            );
            curr_phys += 4096;
        }
    }
}

/// Helper to map an MMIO physical memory range.
pub fn map_mmio(phys_addr: u64, size: usize) {
    let hhdm = hhdm_offset();
    unsafe {
        let active_table_phys = active_cr3();
        let mut page_table = X86_64PageTable::from_root(active_table_phys);
        
        let start_page_phys = phys_addr & !4095;
        let end_page_phys = (phys_addr + size as u64 - 1) & !4095;
        
        let mut curr_phys = start_page_phys;
        while curr_phys <= end_page_phys {
            let curr_virt = curr_phys + hhdm;
            let _ = page_table.map(
                VirtAddr(curr_virt),
                PhysAddr(curr_phys),
                MapFlags::WRITE | MapFlags::NO_CACHE,
            );
            curr_phys += 4096;
        }
    }
}
