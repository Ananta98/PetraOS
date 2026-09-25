//! System calls for signal waiting and suspension: `pause`, `rt_sigpending`, `rt_sigsuspend`.

use crate::ipc::signal::SigSet;
use crate::syscalls::{wrap_syscall, SyscallError, SyscallResult, UserPtr};

/// `sys_pause` (SYS_PAUSE = 34)
/// Suspend calling thread until a signal is delivered.
#[wrap_syscall]
pub fn sys_pause() -> SyscallResult {
    loop {
        if let Some(proc_arc) = crate::proc::current_process() {
            let proc = proc_arc.lock();
            let thread_mask = crate::proc::current_thread()
                .map(|t| t.lock().sig_mask)
                .unwrap_or(0);
            let unblocked = proc.pending_signals.mask & !thread_mask;
            if unblocked != 0 {
                return Err(SyscallError::EINTR);
            }
        }

        crate::arch::enable_and_hlt();

        if let Some(proc_arc) = crate::proc::current_process() {
            let proc = proc_arc.lock();
            let thread_mask = crate::proc::current_thread()
                .map(|t| t.lock().sig_mask)
                .unwrap_or(0);
            let unblocked = proc.pending_signals.mask & !thread_mask;
            if unblocked != 0 {
                return Err(SyscallError::EINTR);
            }
        }
    }
}

/// `sys_rt_sigpending` (SYS_RT_SIGPENDING = 127)
/// Examine pending signals that are blocked from delivery.
#[wrap_syscall]
pub fn sys_rt_sigpending(set_ptr: UserPtr<SigSet>, sigsetsize: usize) -> SyscallResult {
    if sigsetsize != core::mem::size_of::<SigSet>() {
        return Err(SyscallError::EINVAL);
    }
    if set_ptr.is_null() {
        return Err(SyscallError::EFAULT);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let thread_pending = crate::proc::current_thread()
        .map(|t| t.lock().pending_signals.mask)
        .unwrap_or(0);
    let pending = proc.pending_signals.mask | thread_pending;
    drop(proc);

    set_ptr.write(pending).ok_or(SyscallError::EFAULT)?;
    Ok(0)
}

/// `sys_rt_sigsuspend` (SYS_RT_SIGSUSPEND = 130)
/// Atomically replaces signal mask with `mask` and suspends thread until signal.
#[wrap_syscall]
pub fn sys_rt_sigsuspend(mask_ptr: UserPtr<SigSet>, sigsetsize: usize) -> SyscallResult {
    if sigsetsize != core::mem::size_of::<SigSet>() {
        return Err(SyscallError::EINVAL);
    }
    let new_mask = if !mask_ptr.is_null() {
        mask_ptr.read_unaligned().ok_or(SyscallError::EFAULT)?
    } else {
        0
    };

    let thread_arc = crate::proc::current_thread().ok_or(SyscallError::ESRCH)?;
    let old_mask = {
        let mut thread = thread_arc.lock();
        let prev = thread.sig_mask;
        thread.sig_mask = new_mask;
        prev
    };

    loop {
        if let Some(proc_arc) = crate::proc::current_process() {
            let proc = proc_arc.lock();
            let unblocked = proc.pending_signals.mask & !new_mask;
            if unblocked != 0 {
                break;
            }
        }
        crate::arch::enable_and_hlt();
        if let Some(proc_arc) = crate::proc::current_process() {
            let proc = proc_arc.lock();
            let unblocked = proc.pending_signals.mask & !new_mask;
            if unblocked != 0 {
                break;
            }
        }
    }

    thread_arc.lock().sig_mask = old_mask;
    Err(SyscallError::EINTR)
}
