use super::{SyscallError, SyscallResult, is_user_ptr_valid};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::ipc::signal::{SigAction, SigSet, is_uncatchable};

/// `sys_kill` (SYS_KILL = 62)
/// Sends a signal to a process or process group.
pub fn sys_kill(frame: &mut SyscallFrame) -> SyscallResult {
    let pid_raw = frame.arg1() as i32;
    let sig = frame.arg2() as u8;

    log::info!("sys_kill(pid={}, sig={})", pid_raw, sig);
    if sig == 0 || sig > 64 {
        return Err(SyscallError::EINVAL);
    }

    if pid_raw < 0 {
        // Send signal to all processes in process group (-pid_raw)
        let target_pgid = crate::proc::ProcessId((-pid_raw) as u64);
        let procs = crate::proc::find_processes_by_pgid(target_pgid);
        if procs.is_empty() {
            return Err(SyscallError::ESRCH);
        }
        for proc_arc in procs {
            let mut proc = proc_arc.lock();
            let _ = proc.send_signal(sig);
        }
        return Ok(0);
    }

    let target_pid = if pid_raw == 0 {
        let current_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
        current_arc.lock().pid
    } else {
        crate::proc::ProcessId(pid_raw as u64)
    };

    let target_proc = crate::proc::find_process(target_pid).ok_or(SyscallError::ESRCH)?;
    let mut proc = target_proc.lock();
    proc.send_signal(sig).map_err(|_| SyscallError::ESRCH)?;

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

    if !act.is_null() && !is_user_ptr_valid(act as u64, core::mem::size_of::<SigAction>()) {
        return Err(SyscallError::EFAULT);
    }
    if !oact.is_null() && !is_user_ptr_valid(oact as u64, core::mem::size_of::<SigAction>()) {
        return Err(SyscallError::EFAULT);
    }

    let new_action = if !act.is_null() {
        // SAFETY: User pointer validated within Ring 3 address space bounds.
        Some(unsafe { core::ptr::read_volatile(act) })
    } else {
        None
    };

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let mut proc = proc_arc.lock();
    let old_action = proc
        .sigaction(sig, new_action)
        .map_err(|_| SyscallError::EINVAL)?;

    if !oact.is_null() {
        // SAFETY: User pointer validated within Ring 3 address space bounds.
        unsafe {
            core::ptr::write_volatile(oact, old_action);
        }
    }

    Ok(0)
}

/// `sys_rt_sigprocmask` (SYS_RT_SIGPROCMASK = 14)
/// Examine and change blocked signals.
pub fn sys_rt_sigprocmask(frame: &mut SyscallFrame) -> SyscallResult {
    let how = frame.arg1() as i32;
    let set_ptr = frame.arg2() as *const SigSet;
    let oset_ptr = frame.arg3() as *mut SigSet;

    if !set_ptr.is_null() && !is_user_ptr_valid(set_ptr as u64, core::mem::size_of::<SigSet>()) {
        return Err(SyscallError::EFAULT);
    }
    if !oset_ptr.is_null() && !is_user_ptr_valid(oset_ptr as u64, core::mem::size_of::<SigSet>()) {
        return Err(SyscallError::EFAULT);
    }

    let thread_arc = crate::proc::current_thread().ok_or(SyscallError::ESRCH)?;
    let mut thread = thread_arc.lock();

    let set = if !set_ptr.is_null() {
        // SAFETY: User pointer validated within Ring 3 address space bounds.
        unsafe { core::ptr::read_unaligned(set_ptr) }
    } else {
        0
    };

    let old_mask = thread
        .update_sigmask(how, set)
        .map_err(|_| SyscallError::EINVAL)?;

    if !oset_ptr.is_null() {
        // SAFETY: User pointer validated within Ring 3 address space bounds.
        unsafe {
            core::ptr::write_volatile(oset_ptr, old_mask);
        }
    }
    Ok(0)
}

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
