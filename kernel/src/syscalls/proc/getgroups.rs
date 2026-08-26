//! sys_getgroups system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult, UserPtr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::mm::vmm::paging::PageTable;
use crate::proc::ProcessId;


/// `sys_getgroups` (SYS_GETGROUPS = 115)
/// Get list of supplementary group IDs.
pub fn sys_getgroups(frame: &mut SyscallFrame) -> SyscallResult {
    let size = frame.arg1() as i32;
    let list_ptr = UserPtr::<u32>::from_u64(frame.arg2());

    if size < 0 {
        return Err(SyscallError::EINVAL);
    }
    if size == 0 {
        return Ok(1);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let gid = proc.creds.gid;
    drop(proc);

    list_ptr.write(gid).ok_or(SyscallError::EFAULT)?;
    Ok(1)
}
