//! `sys_madvise` system call handler.
//!
//! Handles:
//! - `sys_madvise` (SYS_MADVISE = 28)

use crate::arch::syscall::syscall::SyscallFrame;
use crate::mm::{AddrSpaceError, VirtAddr};
use crate::syscalls::{SyscallError, SyscallResult, USER_SPACE_MAX_ADDR};

/// `sys_madvise` (SYS_MADVISE = 28)
/// Give advice about use of memory.
pub fn sys_madvise(frame: &mut SyscallFrame) -> SyscallResult {
    let addr = frame.arg1() as u64;
    let len = frame.arg2() as usize;
    let advice = frame.arg3() as i32;

    // Address must be page-aligned in Linux madvise
    if (addr & 0xFFF) != 0 {
        return Err(SyscallError::EINVAL);
    }

    if len == 0 {
        return Ok(0);
    }

    if addr.checked_add(len as u64).map_or(true, |end| end > USER_SPACE_MAX_ADDR) {
        return Err(SyscallError::EINVAL);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let mut addr_space = proc.address_space.lock();

    match addr_space.madvise_range(VirtAddr::new(addr), len, advice) {
        Ok(()) => Ok(0),
        Err(AddrSpaceError::UnmappedRange) => Err(SyscallError::ENOMEM),
        Err(AddrSpaceError::InvalidRange) => Err(SyscallError::EINVAL),
        Err(_) => Err(SyscallError::EINVAL),
    }
}
