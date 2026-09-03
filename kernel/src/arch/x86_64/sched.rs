//! Architecture-specific scheduler and context switching routines for x86_64.
//!
//! Provides low-level context switching, hardware register management (FS_BASE, CR3, TSS RSP0),
//! and execution context definitions for the thread scheduler.

use crate::arch::cpu::msr;
use crate::arch::cpu::stack::StackFrame;
use crate::arch::cpu::tss;

// ── Thread CPU Context ────────────────────────────────────────────────────────

/// The architecture-specific execution context (registers and execution state) for x86_64.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct ThreadContext {
    /// Saved Stack Pointer (RSP)
    pub rsp: usize,
    /// Saved Instruction Pointer (RIP)
    pub rip: usize,
    /// Base/Frame Pointer (RBP)
    pub rbp: usize,
    /// Flags register (RFLAGS)
    pub rflags: usize,
    /// Page table base register (CR3) for virtual memory address space
    pub cr3: usize,
    /// Callee-saved general purpose registers
    pub rbx: usize,
    pub r12: usize,
    pub r13: usize,
    pub r14: usize,
    pub r15: usize,
    /// Architecture-specific TLS / Thread Control Block base (FS_BASE)
    pub fs_base: u64,
    /// Architecture-specific Thread Control Block base (GS_BASE)
    pub gs_base: u64,
}

impl Default for ThreadContext {
    fn default() -> Self {
        Self::new()
    }
}

impl ThreadContext {
    /// Creates a new zero-initialized `ThreadContext`.
    pub const fn new() -> Self {
        Self {
            rsp: 0,
            rip: 0,
            rbp: 0,
            rflags: 0x202, // IF (Interrupt Flag) enabled
            cr3: 0,
            rbx: 0,
            r12: 0,
            r13: 0,
            r14: 0,
            r15: 0,
            fs_base: 0,
            gs_base: 0,
        }
    }

    /// Initialize thread context stack frame with entry function and argument.
    pub fn init(&mut self, stack: &mut [u8], entry: extern "C" fn(*mut u8), arg: *mut u8) {
        let rsp = self.init_thread_stack(stack, entry, arg);
        self.rsp = rsp as usize;
        self.rip = thread_entry_trampoline as *const () as usize;
    }

    /// Initialize the stack frame for a new x86_64 thread.
    ///
    /// Configures the stack frame so that upon context switch return, `thread_entry_trampoline`
    /// sets up the System V AMD64 ABI convention:
    /// - Argument in `RDI` (from `r12`)
    /// - Entry point invoked via `call` (from `r13`)
    pub fn init_thread_stack(
        &self,
        stack: &mut [u8],
        entry: extern "C" fn(*mut u8),
        arg: *mut u8,
    ) -> u64 {
        let stack_top = stack.as_ptr() as u64 + stack.len() as u64;
        let stack_top = stack_top & !15; // 16-byte align stack top

        let frame_size = core::mem::size_of::<StackFrame>() as u64;
        let rsp = stack_top - frame_size;
        let frame_ptr = rsp as *mut StackFrame;

        // SAFETY: frame_ptr is within the allocated stack bounds.
        unsafe {
            frame_ptr.write(StackFrame {
                r15: 0,
                r14: 0,
                r13: entry as *const () as u64, // Entry function pointer
                r12: arg as u64,                // Argument passed to entry
                rbx: 0,
                rbp: 0,
                rip: thread_entry_trampoline as *const () as u64,
            });
        }

        rsp
    }
}

// ── Assembly Context Switch & Trampoline Routines ────────────────────────────

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

// ── Low-Level Architecture Context Switch ─────────────────────────────────────

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

/// Architecture CPU idle loop.
pub fn idle() -> ! {
    loop {
        crate::arch::halt();
    }
}
