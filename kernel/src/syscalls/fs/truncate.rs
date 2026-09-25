//! System calls for truncating a file to a specified length (`truncate`, `ftruncate`).

use super::*;
use crate::syscalls::{SyscallError, SyscallResult, UserCStr, wrap_syscall};

/// `sys_truncate` (SYS_TRUNCATE = 76)
/// Truncate a file to a specified length by path.
#[wrap_syscall]
pub fn sys_truncate(path: UserCStr, length: usize) -> SyscallResult {
    let path_str = path.to_string(256)?;
    let full_path = resolve_at_path(AT_FDCWD, &path_str)?;
    crate::fs::truncate(&full_path, length)?;
    Ok(0)
}

/// `sys_ftruncate` (SYS_FTRUNCATE = 77)
/// Truncate an open file descriptor to a specified length.
#[wrap_syscall]
pub fn sys_ftruncate(fd: i32, length: usize) -> SyscallResult {
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
