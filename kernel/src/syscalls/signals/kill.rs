//! sys_kill system call handler.

use crate::syscalls::{SyscallError, SyscallResult};
use crate::arch::syscall::syscall::SyscallFrame;


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
