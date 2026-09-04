//! x86_64 Thread CPU Context and Stack Initialization.
//!
//! Defines the register execution context (`ThreadContext`) and routines to initialize
//! execution stacks compliant with System V AMD64 ABI conventions.

use crate::arch::cpu::stack::StackFrame;
use super::switch::thread_entry_trampoline;

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
