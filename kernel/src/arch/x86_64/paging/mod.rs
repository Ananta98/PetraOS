//! Architecture-Specific Paging Subsystem for x86_64.
//!
//! Provides runtime 4-level and 5-level (LA57) paging detection, TLB invalidation,
//! and the `ArchPageTable` hardware mapper.

pub mod flush;
pub mod helpers;
pub mod table;

pub use helpers::{active_cr3, ensure_mapped, map_mmio, read_cr2};
pub use table::ArchPageTable;

/// Checks if CPU hardware supports 5-level (57-bit) linear address paging via CPUID.(EAX=7, ECX=0):ECX[bit 16].
pub fn supports_5level_paging() -> bool {
    let ecx: u32;
    unsafe {
        core::arch::asm!(
            "push rbx",
            "mov eax, 7",
            "xor ecx, ecx",
            "cpuid",
            "mov {ecx_out:e}, ecx",
            "pop rbx",
            ecx_out = out(reg) ecx,
            out("eax") _,
            out("ecx") _,
            out("edx") _,
            options(nomem, preserves_flags)
        );
    }
    (ecx & (1 << 16)) != 0
}

/// Checks if 5-level paging is currently active on the processor via CR4.LA57 (bit 12).
pub fn is_5level_paging_active() -> bool {
    let cr4 = crate::arch::cpu::read_cr4();
    (cr4 & (1 << 12)) != 0
}

/// Returns the current active number of paging levels (4 or 5).
pub fn active_paging_levels() -> u8 {
    if is_5level_paging_active() {
        5
    } else {
        4
    }
}
