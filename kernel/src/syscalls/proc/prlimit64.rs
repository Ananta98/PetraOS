//! sys_prlimit64 system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult, UserPtr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::mm::vmm::paging::PageTable;
use crate::proc::ProcessId;


/// `sys_prlimit64` (SYS_PRLIMIT64 = 302)
/// Get/set resource limits of an arbitrary process.
pub fn sys_prlimit64(frame: &mut SyscallFrame) -> SyscallResult {
    let _pid = frame.arg1() as i32;
    let resource = frame.arg2() as i32;
    let new_limit_ptr = UserPtr::<RLimit64>::from_u64(frame.arg3());
    let old_limit_ptr = UserPtr::<RLimit64>::from_u64(frame.arg4());

    if !new_limit_ptr.is_null() && !new_limit_ptr.is_valid() {
        return Err(SyscallError::EFAULT);
    }

    if !old_limit_ptr.is_null() {
        let limit = get_default_rlimit(resource);
        old_limit_ptr.write(limit).ok_or(SyscallError::EFAULT)?;
    }

    Ok(0)
}
