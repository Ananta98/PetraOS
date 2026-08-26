//! sys_getpid system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::mm::vmm::paging::PageTable;
use crate::proc::ProcessId;


/// `sys_getpid` (SYS_GETPID = 39)
/// Get process ID.
pub fn sys_getpid(_frame: &mut SyscallFrame) -> SyscallResult {
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    Ok(proc.pid.as_u64() as usize)
}
