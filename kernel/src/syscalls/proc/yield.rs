//! CPU yield system call (`yield`).

use crate::syscalls::{wrap_syscall, SyscallResult};

/// `sys_yield` (SYS_YIELD = 24)
/// Yield the CPU to another runnable thread.
#[wrap_syscall]
pub fn sys_yield() -> SyscallResult {
    crate::proc::thread::Thread::yield_cpu();
    Ok(0)
}

