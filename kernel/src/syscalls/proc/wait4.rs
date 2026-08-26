//! sys_wait4 system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult, UserPtr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::mm::vmm::paging::PageTable;
use crate::proc::ProcessId;


/// `sys_wait4` (SYS_WAIT4 = 61)
/// Wait for process state change.
pub fn sys_wait4(frame: &mut SyscallFrame) -> SyscallResult {
    let pid_raw = frame.arg1() as i32;
    let wstatus = UserPtr::<i32>::from_u64(frame.arg2());
    let options = frame.arg3() as i32;
    let rusage_ptr = UserPtr::<RUsage>::from_u64(frame.arg4());

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
