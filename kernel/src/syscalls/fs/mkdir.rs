//! System calls for directory creation and removal (`mkdir`, `mkdirat`, `rmdir`).

use super::*;
use crate::arch::syscall::syscall::SyscallFrame;
use crate::syscalls::{SyscallResult, UserCStr};

/// `sys_mkdir` (SYS_MKDIR = 83)
/// Create a directory.
pub fn sys_mkdir(frame: &mut SyscallFrame) -> SyscallResult {
    let _mode = frame.arg2() as u32;

    let path = UserCStr::from_u64(frame.arg1()).to_string(256)?;
    let full_path = resolve_at_path(AT_FDCWD, &path)?;
    crate::fs::mkdir(&full_path)?;
    Ok(0)
}

/// `sys_mkdirat` (SYS_MKDIRAT = 258)
/// Create a directory relative to a directory file descriptor.
pub fn sys_mkdirat(frame: &mut SyscallFrame) -> SyscallResult {
    let dfd = frame.arg1() as i32;
    let _mode = frame.arg3() as u32;

    let path = UserCStr::from_u64(frame.arg2()).to_string(256)?;
    let full_path = resolve_at_path(dfd, &path)?;
    crate::fs::mkdir(&full_path)?;
    Ok(0)
}

/// `sys_rmdir` (SYS_RMDIR = 84)
/// Remove an empty directory.
pub fn sys_rmdir(frame: &mut SyscallFrame) -> SyscallResult {
    let path = UserCStr::from_u64(frame.arg1()).to_string(256)?;
    let full_path = resolve_at_path(AT_FDCWD, &path)?;
    crate::fs::rmdir(&full_path)?;
    Ok(0)
}
