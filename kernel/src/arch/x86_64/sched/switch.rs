//! Low-Level Assembly Context Switching and Execution Dispatcher.
//!
//! Provides naked assembly routines for saving/restoring callee-saved registers,
//! thread entry trampoline, and architecture-specific context switching orchestration.

use crate::arch::cpu::msr;
use crate::arch::cpu::tss;

/// Trampoline for newly started kernel threads.
///
/// Under System V AMD64 ABI:
/// - The first argument must be passed in `%rdi`.
///
/// When `switch_context` or `switch_context_to` executes `ret`, it jumps to this trampoline.
/// Registers were popped from `StackFrame`:
/// - `r12`: entry argument (`*mut u8`)
/// - `r13`: entry function pointer (`extern "C" fn(*mut u8)`)
///
/// This trampoline loads `rdi` from `r12` and calls `r13`.
/// If the entry point returns, it enters a safe halt loop.
#[unsafe(naked)]
pub unsafe extern "C" fn thread_entry_trampoline() -> ! {
    core::arch::naked_asm!(
        "mov rdi, r12",
        "call r13",
        // If the entry function returns, halt in a safe idle loop
        "1:",
        "hlt",
        "jmp 1b",
    );
}

/// Saves callee-saved registers of current thread, stores RSP into `*prev_rsp_ptr`,
/// loads next RSP from `next_rsp`, restores callee-saved registers, and returns.
///
/// # Safety
/// `prev_rsp_ptr` must be a valid pointer to store the stack pointer.
/// `next_rsp` must point to a valid stack frame layout (callee-saved registers + return address).
#[unsafe(naked)]
pub unsafe extern "C" fn switch_context(prev_rsp_ptr: *mut u64, next_rsp: u64) {
    core::arch::naked_asm!(
        "push rbp",
        "push rbx",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        "mov [rdi], rsp",
        "mov rsp, rsi",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbx",
        "pop rbp",
        "ret",
    );
}

/// Loads next RSP from `next_rsp`, restores callee-saved registers, and returns
/// into the thread's initial execution entry point without saving previous context.
///
/// # Safety
/// `next_rsp` must point to a valid stack frame layout (callee-saved registers + entry RIP).
#[unsafe(naked)]
pub unsafe extern "C" fn switch_context_to(next_rsp: u64) -> ! {
    core::arch::naked_asm!(
        "mov rsp, rdi",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbx",
        "pop rbp",
        "ret",
    );
}

/// Performs low-level architecture execution context switch between two contexts.
///
/// Handles:
/// 1. Switching address space root (`CR3`) if the target process has a different page table.
/// 2. Restoring thread-local storage (`FS_BASE`) register for the incoming context.
/// 3. Updating TSS `RSP0` for Ring 3 user-to-kernel transitions.
/// 4. Invoking low-level assembly context switch (`switch_context` or `switch_context_to`).
///
/// # Safety
/// - Callers must ensure interrupts are disabled on the local CPU before invoking this function.
/// - `next_rsp` must point to a valid thread execution stack.
/// - If `prev_rsp_ptr` is non-null, it must point to valid memory to store the previous RSP.
pub unsafe fn arch_switch_context(
    prev_rsp_ptr: *mut u64,
    next_rsp: u64,
    next_cr3: u64,
    next_kstack_top: u64,
    next_fs_base: u64,
) {
    // 1. Switch virtual memory address space if needed
    if next_cr3 != 0 {
        let active_cr3 = crate::arch::active_address_space_root();
        if next_cr3 != active_cr3 {
            // SAFETY: next_cr3 is a verified PML4 address space root.
            unsafe {
                crate::arch::set_address_space_root(next_cr3);
            }
        }
    }

    // 2. Restore TLS base register for the incoming thread
    msr::write_fs_base(next_fs_base);

    // 3. Update TSS RSP0 for Ring 3 transitions
    if next_kstack_top != 0 {
        tss::set_rsp0(next_kstack_top);
    }

    // 4. Perform stack and register context switch
    if !prev_rsp_ptr.is_null() {
        // SAFETY: prev_rsp_ptr is non-null and points to valid storage; next_rsp points to a valid stack.
        unsafe { switch_context(prev_rsp_ptr, next_rsp) };
    } else {
        // SAFETY: next_rsp points to an initial valid execution stack.
        unsafe { switch_context_to(next_rsp) };
    }
}
