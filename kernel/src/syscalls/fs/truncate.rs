//! System calls for truncating a file to a specified length (`truncate`, `ftruncate`).

use super::*;
use crate::arch::syscall::syscall::SyscallFrame;
use crate::syscalls::{SyscallError, SyscallResult, UserCStr};

/// `sys_truncate` (SYS_TRUNCATE = 76)
/// Truncate a file to a specified length by path.
pub fn sys_truncate(frame: &mut SyscallFrame) -> SyscallResult {
    let length = frame.arg2() as usize;

    let path = UserCStr::from_u64(frame.arg1()).to_string(256)?;
    let full_path = resolve_at_path(AT_FDCWD, &path)?;
    crate::fs::truncate(&full_path, length)?;
    Ok(0)
}

/// `sys_ftruncate` (SYS_FTRUNCATE = 77)
/// Truncate an open file descriptor to a specified length.
pub fn sys_ftruncate(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let length = frame.arg2() as usize;

    if fd < 0 {
        return Err(SyscallError::EBADF);
    }
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let file = proc.fd_table.get(fd)?;
    drop(proc);

    file.ops
        .truncate(length)
        .or_else(|_| file.dentry.inode.ops.truncate(length))?;
    Ok(0)
}
