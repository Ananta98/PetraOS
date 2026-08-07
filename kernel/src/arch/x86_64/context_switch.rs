// ── Assembly Context Switch & Bootstrap Routines ──────────────────────────────

core::arch::global_asm!(include_str!("Switch.S"));

unsafe extern "C" {
    pub fn switch_context(prev_rsp_ptr: *mut u64, next_rsp: u64);
    pub fn switch_context_to(next_rsp: u64) -> !;
    pub fn thread_bootstrapper() -> !;
}

// ── Stack Frame Layout ────────────────────────────────────────────────────────

/// The layout of the context saved on the thread's stack during a context switch on x86_64.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct StackFrame {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub rbx: u64,
    pub rbp: u64,
    pub rip: u64,
}

/// Initialize the stack frame for a new x86_64 thread.
pub fn init_stack(stack: &mut [u8], entry: extern "C" fn(*mut u8), arg: *mut u8) -> u64 {
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
            r13: arg as u64,   // Argument for entry (stored in callee-saved r13)
            r12: entry as u64, // Entry point function (stored in callee-saved r12)
            rbx: 0,
            rbp: 0,
            rip: thread_bootstrapper as *const () as u64,
        });
    }

    rsp
}
