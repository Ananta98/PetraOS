// ── Stack Frame Layout ────────────────────────────────────────────────────────
use super::context::thread_bootstrapper;
use crate::arch::syscall::SyscallFrame;
use crate::arch::userspace::{USER_CS, USER_DS};
use crate::mm::pmm::PMM;
use crate::mm::{hhdm_offset, PhysAddr, VirtAddr};

core::arch::global_asm!(include_str!("Stack.S"));

unsafe extern "C" {
    pub fn fork_child_return() -> !;
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

/// A page-allocated kernel stack with virtual base and top addresses.
pub struct KernelStack {
    base: VirtAddr,
    top: VirtAddr,
}

impl KernelStack {
    /// Usable kernel stack size (16 KiB = 4 pages, order 2).
    pub const STACK_SIZE: usize = 16 * 1024;
    pub const STACK_ORDER: usize = 2;

    /// Allocate a new page-backed kernel stack from the Physical Memory Manager (PMM).
    pub fn new() -> Result<Self, &'static str> {
        let phys = PMM
            .alloc_pages(Self::STACK_ORDER)
            .ok_or("Failed to allocate physical pages for kernel stack")?;
        let hhdm = hhdm_offset();
        let base = VirtAddr::new(phys.as_u64() + hhdm);
        let top = VirtAddr::new(base.as_u64() + Self::STACK_SIZE as u64);

        // SAFETY: The allocated physical frame is valid and mapped in HHDM.
        unsafe {
            core::ptr::write_bytes(base.as_mut_ptr::<u8>(), 0, Self::STACK_SIZE);
        }

        Ok(Self { base, top })
    }

    /// Returns the 16-byte aligned top virtual address of the stack.
    #[inline(always)]
    pub fn top(&self) -> VirtAddr {
        VirtAddr::new(self.top.as_u64() & !15)
    }

    /// Returns the total guarded/allocated stack size in bytes.
    #[inline(always)]
    pub const fn guarded_size() -> usize {
        Self::STACK_SIZE
    }
}

impl Drop for KernelStack {
    fn drop(&mut self) {
        if !self.base.is_null() {
            let hhdm = hhdm_offset();
            let phys = PhysAddr::new(self.base.as_u64() - hhdm);
            PMM.free_pages(phys, Self::STACK_ORDER);
        }
    }
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

/// Initialize the kernel stack for a child thread created via `fork`.
///
/// Sets up the child's `SyscallFrame` at the top of the stack with `rax = 0` (child return value)
/// and user segment selectors, followed by a `StackFrame` pointing to `fork_child_return`.
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
            rip: fork_child_return as *const () as u64,
        });
    }

    rsp
}
