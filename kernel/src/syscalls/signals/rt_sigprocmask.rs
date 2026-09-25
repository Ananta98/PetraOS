//! sys_rt_sigprocmask system call handler.

use crate::ipc::signal::SigSet;
use crate::syscalls::{wrap_syscall, SyscallError, SyscallResult, UserPtr};

/// `sys_rt_sigprocmask` (SYS_RT_SIGPROCMASK = 14)
/// Examine and change blocked signals.
#[wrap_syscall]
pub fn sys_rt_sigprocmask(
    how: i32,
    set_ptr: UserPtr<SigSet>,
    oset_ptr: UserPtr<SigSet>,
) -> SyscallResult {
    let thread_arc = crate::proc::current_thread().ok_or(SyscallError::ESRCH)?;
    let mut thread = thread_arc.lock();

    let set = if !set_ptr.is_null() {
        set_ptr.read_unaligned().ok_or(SyscallError::EFAULT)?
    } else {
        0
    };

    let old_mask = thread
        .update_sigmask(how, set)
        .map_err(|_| SyscallError::EINVAL)?;

    if !oset_ptr.is_null() {
        oset_ptr.write(old_mask).ok_or(SyscallError::EFAULT)?;
    }
    Ok(0)
}
