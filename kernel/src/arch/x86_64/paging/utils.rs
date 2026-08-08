use crate::mm::{PhysAddr, VirtAddr};
use crate::mm::{MapFlags, PageTable};
use core::sync::atomic::{AtomicU64, Ordering};

/// Returns the Higher Half Direct Map (HHDM) physical-to-virtual offset provided by Limine bootloader.
pub fn hhdm_offset() -> u64 {
    static OFFSET: AtomicU64 = AtomicU64::new(u64::MAX);
    let val = OFFSET.load(Ordering::Relaxed);
    if val != u64::MAX {
        return val;
    }
    let offset = crate::limine::HHDM_REQUEST
        .get_response()
        .expect("Paging: Limine HHDM response is missing")
        .offset();
    OFFSET.store(offset, Ordering::Relaxed);
    offset
}

/// Read the current active physical PML4 root address from control register CR3.
///
/// # Safety
/// Reading register CR3 is safe on x86_64 CPU mode.
pub unsafe fn active_cr3() -> PhysAddr {
    let cr3: u64;
    // SAFETY: Reading CR3 register is required to find the current PML4 physical address.
    unsafe {
        core::arch::asm!("mov {}, cr3", out(reg) cr3);
    }
    PhysAddr(cr3 & !0xFFF)
}

/// Read the faulting virtual address from control register CR2.
///
/// # Safety
/// Reading CR2 returns the page fault linear address.
pub unsafe fn read_cr2() -> VirtAddr {
    let cr2: u64;
    // SAFETY: Reading CR2 register fetches the linear address that caused the latest page fault.
    unsafe {
        core::arch::asm!("mov {}, cr2", out(reg) cr2, options(nomem, nostack, preserves_flags));
    }
    VirtAddr(cr2)
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

/// Helper to ensure a physical memory range is mapped in the active page table.
pub fn ensure_mapped(phys_addr: u64, size: usize) {
    let hhdm = hhdm_offset();
    unsafe {
        let active_table_phys = active_cr3();
        let mut page_table = super::table::ArchPageTable::from_root(active_table_phys);

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
        let mut page_table = super::table::ArchPageTable::from_root(active_table_phys);

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
