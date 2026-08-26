//! sys_fork system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::mm::vmm::paging::PageTable;
use crate::proc::ProcessId;


/// `sys_fork` (SYS_FORK = 57)
/// Fork the current running process and thread context.
pub fn sys_fork(frame: &mut SyscallFrame) -> SyscallResult {
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let child_arc =
        crate::proc::Process::fork(proc_arc, frame).map_err(|_| SyscallError::EAGAIN)?;
    let child_pid = child_arc.lock().pid.as_u64();
    Ok(child_pid as usize)
}
