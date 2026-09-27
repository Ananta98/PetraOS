//! sys_rt_sigaction system call handler.

use crate::syscalls::{SyscallError, SyscallResult, UserPtr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::ipc::signal::{is_uncatchable, SigAction};


/// `sys_rt_sigaction` (SYS_RT_SIGACTION = 13)
/// Examine and change a signal action.
pub fn sys_rt_sigaction(frame: &mut SyscallFrame) -> SyscallResult {
    let sig = frame.arg1() as u8;
    let act = UserPtr::<SigAction>::from_u64(frame.arg2());
    let oact = UserPtr::<SigAction>::from_u64(frame.arg3());
    let sigsetsize = frame.arg4() as usize;

    if sig == 0 || sig > 64 {
        return Err(SyscallError::EINVAL);
    }
    if sigsetsize > 0 && (sigsetsize < core::mem::size_of::<crate::ipc::signal::SigSet>() || sigsetsize > 1024) {
        return Err(SyscallError::EINVAL);
    }
    if is_uncatchable(sig) && !act.is_null() {
        return Err(SyscallError::EINVAL);
    }

    let new_action = if !act.is_null() {
        Some(act.read().ok_or(SyscallError::EFAULT)?)
    } else {
        None
    };

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let mut proc = proc_arc.lock();
    let old_action = proc
        .sigaction(sig, new_action)
        .map_err(|_| SyscallError::EINVAL)?;

    if !oact.is_null() {
        oact.write(old_action).ok_or(SyscallError::EFAULT)?;
        if sigsetsize > core::mem::size_of::<crate::ipc::signal::SigSet>() {
            let extra_len = sigsetsize - core::mem::size_of::<crate::ipc::signal::SigSet>();
            // In SigAction layout, mask is at offset 24 (after handler (8), flags (8), restorer (8))
            let extra_ptr = UserPtr::<u8>::from_u64(frame.arg3() + 24 + core::mem::size_of::<crate::ipc::signal::SigSet>() as u64);
            let slice = extra_ptr.as_slice_mut(extra_len).ok_or(SyscallError::EFAULT)?;
            slice.fill(0);
        }
    }

    Ok(0)
}
