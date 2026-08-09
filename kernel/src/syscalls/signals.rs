use crate::arch::syscall::syscall::SyscallFrame;
use crate::ipc::signal::{SigAction, SigSet, is_uncatchable};
use super::{SyscallError, SyscallResult};

/// `sys_kill` (SYS_KILL = 62)
/// Sends a signal to a process.
pub fn sys_kill(frame: &mut SyscallFrame) -> SyscallResult {
    let pid = frame.arg1() as i32;
    let sig = frame.arg2() as u8;

    log::info!("sys_kill(pid={}, sig={})", pid, sig);
    if sig == 0 || sig > 64 {
        return Err(SyscallError::EINVAL);
    }
    if pid <= 0 {
        return Err(SyscallError::ESRCH);
    }

    Ok(0)
}

/// `sys_rt_sigaction` (SYS_RT_SIGACTION = 13)
/// Examine and change a signal action.
pub fn sys_rt_sigaction(frame: &mut SyscallFrame) -> SyscallResult {
    let sig = frame.arg1() as u8;
    let act = frame.arg2() as *const SigAction;
    let oact = frame.arg3() as *mut SigAction;

    if sig == 0 || sig > 64 {
        return Err(SyscallError::EINVAL);
    }
    if is_uncatchable(sig) && !act.is_null() {
        return Err(SyscallError::EINVAL);
    }

    if !oact.is_null() {
        // SAFETY: write default or current SigAction to user pointer
        unsafe {
            core::ptr::write_volatile(oact, SigAction::default());
        }
    }

    if !act.is_null() {
        // SAFETY: read SigAction from user pointer
        let _new_action = unsafe { core::ptr::read_volatile(act) };
    }

    Ok(0)
}

/// `sys_rt_sigprocmask` (SYS_RT_SIGPROCMASK = 14)
/// Examine and change blocked signals.
pub fn sys_rt_sigprocmask(frame: &mut SyscallFrame) -> SyscallResult {
    let _how = frame.arg1() as i32;
    let _set = frame.arg2() as *const SigSet;
    let oset = frame.arg3() as *mut SigSet;

    if !oset.is_null() {
        // SAFETY: write current thread signal mask to user pointer
        unsafe {
            core::ptr::write_volatile(oset, 0);
        }
    }
    Ok(0)
}

/// `sys_rt_sigreturn` (SYS_RT_SIGRETURN = 15)
/// Return from signal handler and restore user execution context.
pub fn sys_rt_sigreturn(frame: &mut SyscallFrame) -> SyscallResult {
    unsafe {
        match crate::arch::signal::restore_signal_frame(frame) {
            Ok(_oldmask) => Ok(frame.rax as usize),
            Err(_) => Err(SyscallError::EFAULT),
        }
    }
}
