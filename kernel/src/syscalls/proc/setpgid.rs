//! sys_setpgid system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::mm::vmm::paging::PageTable;
use crate::proc::ProcessId;


/// `sys_setpgid` (SYS_SETPGID = 109)
/// Set process group ID.
pub fn sys_setpgid(frame: &mut SyscallFrame) -> SyscallResult {
    let pid_raw = frame.arg1() as i32;
    let pgid_raw = frame.arg2() as i32;

    let target_pid = if pid_raw <= 0 {
        let current_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
        current_arc.lock().pid
    } else {
        ProcessId(pid_raw as u64)
    };

    let target_proc = crate::proc::find_process(target_pid).ok_or(SyscallError::ESRCH)?;
    let mut proc = target_proc.lock();

    let new_pgid = if pgid_raw <= 0 {
        proc.pid
    } else {
        ProcessId(pgid_raw as u64)
    };

    proc.pgid = new_pgid;
    Ok(0)
}
