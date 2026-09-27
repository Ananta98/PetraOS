//! sys_rt_sigprocmask system call handler.

use crate::syscalls::{SyscallError, SyscallResult, UserPtr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::ipc::signal::SigSet;


/// `sys_rt_sigprocmask` (SYS_RT_SIGPROCMASK = 14)
/// Examine and change blocked signals.
pub fn sys_rt_sigprocmask(frame: &mut SyscallFrame) -> SyscallResult {
    let how = frame.arg1() as i32;
    let set_ptr = UserPtr::<SigSet>::from_u64(frame.arg2());
    let oset_ptr = UserPtr::<SigSet>::from_u64(frame.arg3());
    let sigsetsize = frame.arg4() as usize;

    if sigsetsize > 0 && (sigsetsize < core::mem::size_of::<SigSet>() || sigsetsize > 1024) {
        return Err(SyscallError::EINVAL);
    }

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
        if sigsetsize > core::mem::size_of::<SigSet>() {
            let extra_len = sigsetsize - core::mem::size_of::<SigSet>();
            let extra_ptr = UserPtr::<u8>::from_u64(frame.arg3() + core::mem::size_of::<SigSet>() as u64);
            let slice = extra_ptr.as_slice_mut(extra_len).ok_or(SyscallError::EFAULT)?;
            slice.fill(0);
        }
    }
    Ok(0)
}
