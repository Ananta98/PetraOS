//! sys_setrlimit system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult, UserPtr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::mm::vmm::paging::PageTable;
use crate::proc::ProcessId;


/// `sys_setrlimit` (SYS_SETRLIMIT = 160)
/// Set resource limits.
pub fn sys_setrlimit(frame: &mut SyscallFrame) -> SyscallResult {
    let _resource = frame.arg1() as i32;
    let rlim_ptr = UserPtr::<RLimit64>::from_u64(frame.arg2());

    if !rlim_ptr.is_valid() {
        return Err(SyscallError::EFAULT);
    }
    Ok(0)
}
