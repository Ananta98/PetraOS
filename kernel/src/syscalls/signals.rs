use super::{SyscallError, SyscallResult, UserPtr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::ipc::signal::{is_uncatchable, SigAction, SigSet};

/// `sys_kill` (SYS_KILL = 62)
/// Sends a signal to a process or process group.
pub fn sys_kill(frame: &mut SyscallFrame) -> SyscallResult {
    let pid_raw = frame.arg1() as i32;
    let sig = frame.arg2() as u8;

    log::debug!("sys_kill(pid={}, sig={})", pid_raw, sig);
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
    let act = UserPtr::<SigAction>::from_u64(frame.arg2());
    let oact = UserPtr::<SigAction>::from_u64(frame.arg3());

    if sig == 0 || sig > 64 {
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
    }

    Ok(0)
}

/// `sys_rt_sigprocmask` (SYS_RT_SIGPROCMASK = 14)
/// Examine and change blocked signals.
pub fn sys_rt_sigprocmask(frame: &mut SyscallFrame) -> SyscallResult {
    let how = frame.arg1() as i32;
    let set_ptr = UserPtr::<SigSet>::from_u64(frame.arg2());
    let oset_ptr = UserPtr::<SigSet>::from_u64(frame.arg3());

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
