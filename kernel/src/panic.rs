//! Kernel Panic Subsystem & Stack Trace Unwinding.
//!
//! Provides the centralized panic handler for PetraOS, formatting diagnostic output,
//! active CPU/process context, and frame-pointer-based stack unwinding before halting.

use core::panic::PanicInfo;

/// Walk the stack frame pointers starting from `rbp` and print return addresses.
pub fn print_stack_trace_from(mut rbp: u64) {
    log::error!("Stack backtrace:");
    let mut frame_idx = 0;

    // Walk frame pointers (capped at 32 frames to avoid infinite loops on corrupted stacks)
    while rbp != 0 && frame_idx < 32 {
        // Frame pointer validation:
        // 1. Must be 8-byte aligned.
        // 2. Must not be null or within zero page (< 0x1000).
        // 3. Must be a canonical 64-bit address.
        if rbp % 8 != 0
            || rbp < 0x1000
            || (rbp >= 0x0000_8000_0000_0000 && rbp < 0xffff_8000_0000_0000)
        {
            break;
        }

        // SAFETY: Pointer is 8-byte aligned and checked against canonical memory bounds.
        let saved_rbp = unsafe { *(rbp as *const u64) };
        let return_addr = unsafe { *((rbp as *const u64).add(1)) };

        if return_addr == 0 {
            break;
        }

        log::error!(
            "  {:>2}: [{:#018x}] (frame: {:#018x})",
            frame_idx,
            return_addr,
            rbp
        );

        // Stack grows downwards, so caller's saved RBP must be strictly greater than current RBP
        if saved_rbp <= rbp && saved_rbp != 0 {
            break;
        }

        rbp = saved_rbp;
        frame_idx += 1;
    }

    if frame_idx == 0 {
        log::error!("  (No frame pointer backtrace available)");
    }
}

/// Capture the current CPU frame pointer (RBP) and print the active call stack.
pub fn print_stack_trace() {
    let rbp: u64;
    // SAFETY: Reading the RBP register has no memory side-effects.
    unsafe {
        core::arch::asm!("mov {}, rbp", out(reg) rbp, options(nomem, nostack, preserves_flags));
    }
    print_stack_trace_from(rbp);
}

/// Central panic handler for the PetraOS kernel.
#[panic_handler]
pub fn rust_panic(info: &PanicInfo) -> ! {
    crate::arch::without_interrupts(|| {
        log::error!("==================== KERNEL PANIC ====================");

        if let Some(location) = info.location() {
            log::error!(
                "Panic Location: {}:{}:{}",
                location.file(),
                location.line(),
                location.column()
            );
        }

        log::error!("Message: {}", info.message());

        log::error!("------------------------------------------------------");
        print_stack_trace();
        log::error!("======================================================");
    });

    crate::arch::idle()
}
