//! sys_rt_sigreturn system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::ipc::signal::{is_uncatchable, SigAction, SigSet};


/// `sys_rt_sigreturn` (SYS_RT_SIGRETURN = 15)
/// Return from signal handler and restore user execution context.
pub fn sys_rt_sigreturn(frame: &mut SyscallFrame) -> SyscallResult {
    // SAFETY: Restores user stack signal frame.
    unsafe {
        match crate::arch::signal::restore_signal_frame(frame) {
            Ok(_oldmask) => Ok(frame.rax as usize),
            Err(_) => Err(SyscallError::EFAULT),
        }
    }
}
