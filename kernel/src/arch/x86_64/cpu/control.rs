//! CPU Execution Control, Core Identification, and Control Register Operations.
//!
//! Provides halt/idle routines, Local APIC core ID queries, system-wide CPU core count,
//! and direct CR0/CR2/CR3/CR4 register read/write operations.

use core::arch::asm;
use crate::panic::StackFrame;

/// Halt CPU until the next interrupt (`hlt`).
#[inline(always)]
pub fn halt() {
    // SAFETY: Executing hlt instruction to enter low-power state until next interrupt.
    unsafe {
        asm!("hlt", options(nomem, nostack, preserves_flags));
    }
}

/// Atomically enable interrupts and halt CPU until the next interrupt (`sti; hlt`).
#[inline(always)]
pub fn enable_and_hlt() {
    // SAFETY: Atomically enables interrupts and halts the CPU to wait for the next interrupt.
    unsafe {
        asm!("sti", "hlt", options(nomem, nostack, preserves_flags));
    }
}

/// CPU idle loop.
///
/// Continuously puts the CPU into a low-power halt state until the next interrupt.
pub fn idle() -> ! {
    loop {
        halt();
    }
}

/// Get the Local APIC ID of the calling CPU core (defaults to 0 if APIC not yet initialized).
pub fn cpu_id() -> u32 {
    // SAFETY: Queries the initialized Local APIC or defaults to core 0.
    unsafe {
        crate::arch::interrupt::lapic::try_get_lapic()
            .map(|l| l.id())
            .unwrap_or(0)
    }
}

/// Returns the total number of CPU cores detected on the system.
pub fn cpu_count() -> u32 {
    crate::limine::MP_REQUEST
        .get_response()
        .map(|r| r.cpus().len() as u32)
        .unwrap_or(1)
        .max(1)
}

// ── Control Register Operations ──────────────────────────────────────────────

/// Read Control Register 0 (CR0).
#[inline(always)]
pub fn read_cr0() -> u64 {
    let val: u64;
    unsafe {
        asm!("mov {}, cr0", out(reg) val, options(nomem, nostack, preserves_flags));
    }
    val
}

/// Write Control Register 0 (CR0).
#[inline(always)]
pub unsafe fn write_cr0(val: u64) {
    unsafe {
        asm!("mov cr0, {}", in(reg) val, options(nomem, nostack, preserves_flags));
    }
}

/// Read Control Register 2 (CR2) — Linear address of the last page fault.
#[inline(always)]
pub fn read_cr2() -> u64 {
    let val: u64;
    unsafe {
        asm!("mov {}, cr2", out(reg) val, options(nomem, nostack, preserves_flags));
    }
    val
}

/// Read Control Register 3 (CR3) — Page table root directory physical address.
#[inline(always)]
pub fn read_cr3() -> u64 {
    let val: u64;
    unsafe {
        asm!("mov {}, cr3", out(reg) val, options(nomem, nostack, preserves_flags));
    }
    val
}

/// Write Control Register 3 (CR3).
#[inline(always)]
pub unsafe fn write_cr3(val: u64) {
    unsafe {
        asm!("mov cr3, {}", in(reg) val, options(nomem, nostack, preserves_flags));
    }
}

/// Read Control Register 4 (CR4).
#[inline(always)]
pub fn read_cr4() -> u64 {
    let val: u64;
    unsafe {
        asm!("mov {}, cr4", out(reg) val, options(nomem, nostack, preserves_flags));
    }
    val
}

/// Write Control Register 4 (CR4).
#[inline(always)]
pub unsafe fn write_cr4(val: u64) {
    unsafe {
        asm!("mov cr4, {}", in(reg) val, options(nomem, nostack, preserves_flags));
    }
}

/// Read the current CPU base/frame pointer (RBP register).
#[inline(always)]
pub fn read_frame_pointer() -> *const StackFrame {
    let rbp: *const StackFrame;
    // SAFETY: Reading the RBP register produces the current activation frame
    // and has no side effects on processor state or memory.
    unsafe {
        core::arch::asm!(
            "mov {}, rbp",
            out(reg) rbp,
            options(nomem, nostack, preserves_flags)
        );
    }
    rbp
}

/// Sets the active page table physical root address (CR3).
///
/// # Safety
/// The caller must ensure `root` points to a valid root page table (PML4/PML5) physical address.
#[inline(always)]
pub unsafe fn set_address_space_root(root: u64) {
    unsafe {
        write_cr3(root);
    }
}

/// Returns the current active page table physical root address (CR3).
#[inline(always)]
pub fn active_address_space_root() -> u64 {
    read_cr3() & 0x000F_FFFF_FFFF_F000
}
