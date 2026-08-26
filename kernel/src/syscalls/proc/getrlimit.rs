//! sys_getrlimit system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult, UserPtr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::mm::vmm::paging::PageTable;
use crate::proc::ProcessId;


/// `sys_getrlimit` (SYS_GETRLIMIT = 97)
/// Get resource limits.
pub fn sys_getrlimit(frame: &mut SyscallFrame) -> SyscallResult {
    let resource = frame.arg1() as i32;
    let rlim_ptr = UserPtr::<RLimit64>::from_u64(frame.arg2());

    let limit = get_default_rlimit(resource);
    rlim_ptr.write(limit).ok_or(SyscallError::EFAULT)?;
    Ok(0)
}
