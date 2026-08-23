//! Paging Helper Functions for x86_64 Architecture.
//!
//! Provides routines for reading control registers, toggling NXE, and mapping MMIO ranges.

use super::table::ArchPageTable;
use crate::arch::cpu::msr::{rdmsr, wrmsr, IA32_EFER};
use crate::arch::{active_address_space_root, read_cr2 as arch_read_cr2};
use crate::mm::hhdm_offset;
use crate::mm::{PageTable, PageTableFlags, PhysAddr, VirtAddr};

/// Read the current active physical page table root address (CR3).
#[inline(always)]
pub fn active_cr3() -> PhysAddr {
    PhysAddr::new(active_address_space_root())
}

/// Read the linear faulting virtual address (CR2).
#[inline(always)]
pub fn read_cr2() -> VirtAddr {
    VirtAddr::new(arch_read_cr2())
}

/// Enable the No-Execute (NXE) bit (bit 11) in the IA32_EFER MSR.
pub unsafe fn enable_nxe() {
    unsafe {
        let efer = rdmsr(IA32_EFER);
        wrmsr(IA32_EFER, efer | (1 << 11));
    }
}

/// Helper to ensure a physical memory range is mapped in the active page table.
pub fn ensure_mapped(phys_addr: u64, size: usize) {
    if size == 0 {
        return;
    }
    let hhdm = hhdm_offset();
    unsafe {
        let active_table_phys = PhysAddr::new(active_address_space_root());
        let mut page_table = ArchPageTable::from_root(active_table_phys);

        let start_page_phys = phys_addr & !4095;
        let end_page_phys = (phys_addr + size as u64 - 1) & !4095;

        let mut curr_phys = start_page_phys;
        while curr_phys <= end_page_phys {
            let curr_virt = curr_phys + hhdm;
            let _ = page_table.map(
                VirtAddr::new(curr_virt),
                PhysAddr::new(curr_phys),
                PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
            );
            curr_phys += 4096;
        }
    }
}

/// Helper to map an MMIO physical memory range.
pub fn map_mmio(phys_addr: u64, size: usize) {
    if size == 0 {
        return;
    }
    let hhdm = hhdm_offset();
    unsafe {
        let active_table_phys = PhysAddr::new(active_address_space_root());
        let mut page_table = ArchPageTable::from_root(active_table_phys);

        let start_page_phys = phys_addr & !4095;
        let end_page_phys = (phys_addr + size as u64 - 1) & !4095;

        let mut curr_phys = start_page_phys;
        while curr_phys <= end_page_phys {
            let curr_virt = curr_phys + hhdm;
            let _ = page_table.map(
                VirtAddr::new(curr_virt),
                PhysAddr::new(curr_phys),
                PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_CACHE,
            );
            curr_phys += 4096;
        }
    }
}
