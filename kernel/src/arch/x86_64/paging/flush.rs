//! Translation Lookaside Buffer (TLB) Invalidation for x86_64.
//!
//! Provides native single-page (`invlpg`) and complete TLB flushes via CR3 reloads.

use core::arch::asm;
use crate::mm::VirtAddr;

/// Invalidate the TLB entry for the given virtual address.
#[inline(always)]
pub fn invlpg(addr: VirtAddr) {
    // SAFETY: Invalidating a single TLB entry is safe on x86_64.
    unsafe {
        asm!("invlpg [{}]", in(reg) addr.as_u64(), options(nostack, preserves_flags));
    }
}

/// Flush the entire Translation Lookaside Buffer by reloading the active CR3 register.
#[inline(always)]
pub fn flush_all() {
    // SAFETY: Reloading CR3 invalidates all non-global TLB entries.
    unsafe {
        asm!(
            "mov {tmp}, cr3",
            "mov cr3, {tmp}",
            tmp = out(reg) _,
            options(nomem, nostack, preserves_flags)
        );
    }
}
