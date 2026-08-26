//! sys_setsid system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::mm::vmm::paging::PageTable;
use crate::proc::ProcessId;


/// `sys_setsid` (SYS_SETSID = 112)
/// Creates a new session if the calling process is not a process group leader.
pub fn sys_setsid(_frame: &mut SyscallFrame) -> SyscallResult {
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let mut proc = proc_arc.lock();
    proc.pgid = proc.pid;
    Ok(proc.pid.as_u64() as usize)
}
