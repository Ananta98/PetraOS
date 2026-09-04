//! Kernel Panic Subsystem & Stack Trace Unwinding.
//!
//! Provides the centralized panic handler for PetraOS, formatting diagnostic output,
//! CPU core identification, and frame-pointer-based stack unwinding before halting.

use core::panic::PanicInfo;
use core::sync::atomic::{AtomicBool, Ordering};

/// Maximum number of stack frames to traverse before terminating the backtrace.
pub const MAX_STACK_FRAMES: usize = 32;

/// Panic re-entrancy guard to detect and handle nested panics or multi-core contention.
static PANICKING: AtomicBool = AtomicBool::new(false);

/// Standard x86_64 call frame layout created by compiler function prologues
/// when frame pointers (`-Cforce-frame-pointers=yes`) are preserved.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct StackFrame {
    /// Pointer to the caller's saved base frame (previous RBP).
    pub prev: *const StackFrame,
    /// Return address into caller's instruction sequence.
    pub return_addr: u64,
}

impl StackFrame {
    /// Validates whether a pointer points to a plausible, readable stack frame.
    ///
    /// Checks that the pointer:
    /// 1. Is not null and resides beyond the zero page (`>= 0x1000`).
    /// 2. Is aligned to [`core::mem::align_of::<StackFrame>()`] (8 bytes).
    /// 3. Falls within valid 64-bit canonical address space bounds.
    #[inline]
    pub fn is_valid_ptr(ptr: *const Self) -> bool {
        let addr = ptr as u64;

        if addr < 0x1000 {
            return false;
        }

        if (ptr as usize) % core::mem::align_of::<Self>() != 0 {
            return false;
        }

        // In x86_64, canonical addresses have bits 47..63 sign-extended.
        // Addresses in [0x0000_8000_0000_0000, 0xffff_8000_0000_0000) are non-canonical.
        if (0x0000_8000_0000_0000..0xffff_8000_0000_0000).contains(&addr) {
            return false;
        }

        true
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

/// Walk the stack frame chain starting from a given frame pointer address.
pub fn print_stack_trace_from(frame_ptr: u64) {
    let mut curr = frame_ptr as *const StackFrame;
    let mut frame_count = 0;

    log::error!("Stack backtrace (most recent call first):");

    while frame_count < MAX_STACK_FRAMES && StackFrame::is_valid_ptr(curr) {
        // SAFETY: `curr` is verified to be non-null, 8-byte aligned, outside the zero-page,
        // and within canonical 64-bit address space bounds.
        let frame = unsafe { &*curr };
        let ret_addr = frame.return_addr;

        if ret_addr == 0 {
            break;
        }

        log::error!(
            "  {:>2}: [{:#018x}] (frame: {:p})",
            frame_count,
            ret_addr,
            curr
        );

        // In the x86_64 SysV ABI, the stack grows downwards towards lower addresses.
        // Therefore, the caller's frame pointer (`prev`) must be strictly higher in memory.
        // If `prev <= curr`, the stack has looped or encountered corruption.
        let prev = frame.prev;
        if prev <= curr {
            break;
        }

        curr = prev;
        frame_count += 1;
    }

    if frame_count == 0 {
        log::error!("  (No frame pointer backtrace available)");
    } else if frame_count == MAX_STACK_FRAMES {
        log::error!("  ... backtrace truncated at {} frames", MAX_STACK_FRAMES);
    }
}

/// Capture the current CPU frame pointer and print the active call stack.
pub fn print_stack_trace() {
    let fp = read_frame_pointer();
    print_stack_trace_from(fp as u64);
}

/// Central panic handler for the PetraOS kernel.
#[panic_handler]
pub fn rust_panic(info: &PanicInfo) -> ! {
    // Unconditionally disable interrupts so timers or hardware devices do not preempt panic logging.
    crate::arch::disable_interrupts();

    let cpu_id = crate::arch::cpu_id();

    // Guard against nested/recursive panics (e.g., if logging or unwinding faults).
    if PANICKING.swap(true, Ordering::SeqCst) {
        log::error!(
            "Nested panic detected on CPU core #{} — halting execution.",
            cpu_id
        );
        crate::arch::idle();
    }

    log::error!("======================= KERNEL PANIC =======================");
    log::error!("CPU Core: #{}", cpu_id);

    if let Some(location) = info.location() {
        log::error!(
            "Location: {}:{}:{}",
            location.file(),
            location.line(),
            location.column()
        );
    }

    log::error!("Reason:   {}", info.message());
    log::error!("------------------------------------------------------------");

    print_stack_trace();

    log::error!("============================================================");
    log::error!("System halted.");

    crate::arch::idle()
}
