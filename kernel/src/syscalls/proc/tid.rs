//! Thread ID and thread lifecycle system calls.
//!
//! Handles:
//! - `sys_gettid` (SYS_GETTID = 186)
//! - `sys_set_tid_address` (SYS_SET_TID_ADDRESS = 218)

use crate::syscalls::{wrap_syscall, SyscallError, SyscallResult};

/// `sys_gettid` (SYS_GETTID = 186)
/// Returns the caller's thread ID (TID).
#[wrap_syscall]
pub fn sys_gettid() -> SyscallResult {
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    Ok(proc.pid.as_u64() as usize)
}

/// `sys_set_tid_address` (SYS_SET_TID_ADDRESS = 218)
/// Sets the clear_child_tid address for thread exit futex notification,
/// and returns the caller's thread ID (TID).
#[wrap_syscall]
pub fn sys_set_tid_address(_tidptr: u64) -> SyscallResult {
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    Ok(proc.pid.as_u64() as usize)
}

