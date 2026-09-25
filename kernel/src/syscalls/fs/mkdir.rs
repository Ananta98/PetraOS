//! System calls for directory creation and removal (`mkdir`, `mkdirat`, `rmdir`).

use super::*;
use crate::syscalls::{wrap_syscall, SyscallResult, UserCStr};

/// `sys_mkdir` (SYS_MKDIR = 83)
/// Create a directory.
#[wrap_syscall]
pub fn sys_mkdir(path_ptr: UserCStr, _mode: u32) -> SyscallResult {
    let path = path_ptr.to_string(256)?;
    let full_path = resolve_at_path(AT_FDCWD, &path)?;
    crate::fs::mkdir(&full_path)?;
    Ok(0)
}

/// `sys_mkdirat` (SYS_MKDIRAT = 258)
/// Create a directory relative to a directory file descriptor.
#[wrap_syscall]
pub fn sys_mkdirat(dfd: i32, path_ptr: UserCStr, _mode: u32) -> SyscallResult {
    let path = path_ptr.to_string(256)?;
    let full_path = resolve_at_path(dfd, &path)?;
    crate::fs::mkdir(&full_path)?;
    Ok(0)
}

/// `sys_rmdir` (SYS_RMDIR = 84)
/// Remove an empty directory.
#[wrap_syscall]
pub fn sys_rmdir(path_ptr: UserCStr) -> SyscallResult {
    let path = path_ptr.to_string(256)?;
    let full_path = resolve_at_path(AT_FDCWD, &path)?;
    crate::fs::rmdir(&full_path)?;
    Ok(0)
}
