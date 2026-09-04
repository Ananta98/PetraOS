//! CPU Interrupt Flag Management (RFLAGS.IF, sti, cli).
//!
//! Provides architecture-level control over CPU interrupt delivery, checking IF,
//! and scoped interrupt disabling.

use core::arch::asm;

/// Enable interrupts on the calling CPU (`sti`).
#[inline(always)]
pub fn enable_interrupts() {
    // SAFETY: Enabling interrupts is safe when handlers and IDT are set up.
    unsafe {
        asm!("sti", options(nomem, nostack, preserves_flags));
    }
}

/// Disable interrupts on the calling CPU (`cli`) and return the previous interrupt flag state.
#[inline(always)]
pub fn disable_interrupts() -> bool {
    let was_enabled = interrupts_enabled();
    // SAFETY: Disabling interrupts is an atomic operation on the current core.
    unsafe {
        asm!("cli", options(nomem, nostack, preserves_flags));
    }
    was_enabled
}

/// Check if interrupts are currently enabled on the calling CPU.
#[inline(always)]
pub fn interrupts_enabled() -> bool {
    let rflags: u64;
    // SAFETY: Reading RFLAGS to check the IF (Interrupt Flag, bit 9).
    unsafe {
        asm!("pushfq", "pop {}", out(reg) rflags, options(nomem, preserves_flags));
    }
    (rflags & (1 << 9)) != 0
}

/// Execute a closure with interrupts disabled, restoring the previous interrupt state afterwards.
#[inline(always)]
pub fn without_interrupts<R>(func: impl FnOnce() -> R) -> R {
    let was_enabled = disable_interrupts();
    let result = func();
    if was_enabled {
        enable_interrupts();
    }
    result
}
