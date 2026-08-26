//! sys_setgid system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::mm::vmm::paging::PageTable;
use crate::proc::ProcessId;


/// `sys_setgid` (SYS_SETGID = 106)
/// Set group ID.
pub fn sys_setgid(frame: &mut SyscallFrame) -> SyscallResult {
    let gid = frame.arg1() as u32;
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let mut proc = proc_arc.lock();
    let creds = alloc::sync::Arc::make_mut(&mut proc.creds);
    creds.gid = gid;
    creds.egid = gid;
    Ok(0)
}
