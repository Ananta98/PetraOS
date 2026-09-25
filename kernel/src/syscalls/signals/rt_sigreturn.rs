//! sys_rt_sigreturn system call handler.

use crate::syscalls::{wrap_syscall, SyscallError, SyscallResult};

/// `sys_rt_sigreturn` (SYS_RT_SIGRETURN = 15)
/// Return from signal handler and restore user execution context.
#[wrap_syscall]
pub fn sys_rt_sigreturn(frame: &mut SyscallFrame) -> SyscallResult {
    // SAFETY: Restores user stack signal frame.
    unsafe {
        match crate::arch::signal::restore_signal_frame(frame) {
            Ok(_oldmask) => Ok(frame.rax as usize),
            Err(_) => Err(SyscallError::EFAULT),
        }
    }
}
