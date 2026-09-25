//! Process wait system call (`wait4`).

use super::*;
use crate::syscalls::{wrap_syscall, SyscallError, SyscallResult, UserPtr};

/// `sys_wait4` (SYS_WAIT4 = 61)
/// Wait for process state change.
#[wrap_syscall]
pub fn sys_wait4(
    pid_raw: i32,
    wstatus: UserPtr<i32>,
    options: i32,
    rusage_ptr: UserPtr<RUsage>,
) -> SyscallResult {
    let wnohang = (options & 1) != 0;
    let wuntraced = (options & 2) != 0;

    let (child_pid, status) = loop {
        let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
        let mut proc = proc_arc.lock();

        match proc.try_wait4(pid_raw, wuntraced)? {
            Some(res) => {
                drop(proc);
                break res;
            }
            None => {
                drop(proc);
                if wnohang {
                    break (crate::proc::ProcessId(0), 0);
                }
                crate::proc::thread::Thread::yield_cpu();
            }
        }
    };

    if !wstatus.is_null() {
        wstatus.write(status).ok_or(SyscallError::EFAULT)?;
    }

    if !rusage_ptr.is_null() {
        rusage_ptr.write(RUsage::default()).ok_or(SyscallError::EFAULT)?;
    }

    Ok(child_pid.as_u64() as usize)
}
