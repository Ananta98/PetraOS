//! CPU Execution Control and Core Identification.
//!
//! Provides routines for halting the processor, combined enable-and-halt,
//! Local APIC core ID queries, and system-wide CPU core count detection.

use core::arch::asm;

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
