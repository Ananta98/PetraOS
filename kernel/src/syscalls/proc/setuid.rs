//! sys_setuid system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::mm::vmm::paging::PageTable;
use crate::proc::ProcessId;


/// `sys_setuid` (SYS_SETUID = 105)
/// Set user ID.
pub fn sys_setuid(frame: &mut SyscallFrame) -> SyscallResult {
    let uid = frame.arg1() as u32;
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let mut proc = proc_arc.lock();
    let creds = alloc::sync::Arc::make_mut(&mut proc.creds);
    creds.uid = uid;
    creds.euid = uid;
    Ok(0)
}
