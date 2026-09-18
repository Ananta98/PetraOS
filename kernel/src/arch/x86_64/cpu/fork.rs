//! Fork Return Trampoline and Stack Setup for x86_64.
//!
//! Provides the `fork_return` naked function, the `StackFrame` context layout
//! used during context switches, and `init_fork_stack` which constructs the
//! child thread's initial kernel stack for POSIX `fork()`.

use crate::arch::syscall::SyscallFrame;
use crate::arch::userspace::{USER_CS, USER_DS};
use crate::proc::KernelStack;

/// Restores general purpose registers up to r15 from `SyscallFrame`,
/// restores user GS base via `swapgs`, and returns to user space via `iretq`.
#[unsafe(naked)]
pub unsafe extern "C" fn fork_return() -> ! {
    core::arch::naked_asm!(
        "pop r15", "pop r14", "pop r13", "pop r12", "pop r11", "pop r10", "pop r9", "pop r8",
        "pop rbp", "pop rdi", "pop rsi", "pop rdx", "pop rcx", "pop rbx", "pop rax", "swapgs",
        "iretq",
    );
}

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

/// Initialize the kernel stack for a child thread created via `fork`.
///
/// Sets up the child's `SyscallFrame` at the top of the stack with `rax = 0` (child return value)
/// and user segment selectors, followed by a `StackFrame` pointing to `fork_return`.
pub fn init_fork_stack(kstack: &mut KernelStack, parent_frame: &SyscallFrame) -> u64 {
    let kstack_top = kstack.top().as_u64();
    let syscall_frame_size = core::mem::size_of::<SyscallFrame>() as u64;
    let syscall_frame_ptr = (kstack_top - syscall_frame_size) as *mut SyscallFrame;

    let mut child_frame = *parent_frame;
    // POSIX fork: child process receives return value 0
    child_frame.rax = 0;
    child_frame.cs = USER_CS; // User code segment (RPL=3)
    child_frame.ss = USER_DS; // User data segment (RPL=3)

    // SAFETY: Writing child SyscallFrame within allocated KernelStack bounds.
    unsafe {
        syscall_frame_ptr.write(child_frame);
    }

    let stack_frame_size = core::mem::size_of::<StackFrame>() as u64;
    let rsp = (syscall_frame_ptr as u64) - stack_frame_size;
    let frame_ptr = rsp as *mut StackFrame;

    // SAFETY: Writing StackFrame within allocated KernelStack bounds.
    unsafe {
        frame_ptr.write(StackFrame {
            r15: 0,
            r14: 0,
            r13: 0,
            r12: 0,
            rbx: 0,
            rbp: 0,
            rip: fork_return as *const () as u64,
        });
    }

    rsp
}
