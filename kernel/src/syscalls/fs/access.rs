//! System calls for checking file accessibility and permissions (`access`, `faccessat`).

use super::*;
use crate::arch::syscall::syscall::SyscallFrame;
use crate::syscalls::{SyscallError, SyscallResult, UserCStr};

/// `sys_access` (SYS_ACCESS = 21)
/// Check user's permissions for a file.
pub fn sys_access(frame: &mut SyscallFrame) -> SyscallResult {
    let mode = frame.arg2() as i32;

    if !(0..=7).contains(&mode) {
        return Err(SyscallError::EINVAL);
    }

    let path = UserCStr::from_u64(frame.arg1()).to_string(256)?;
    let full_path = resolve_at_path(AT_FDCWD, &path)?;
    let _dentry = crate::fs::resolve_path(&full_path)?;

    Ok(0)
}

/// `sys_faccessat` (SYS_FACCESSAT = 269)
/// Check user's permissions for a file relative to a directory file descriptor.
pub fn sys_faccessat(frame: &mut SyscallFrame) -> SyscallResult {
    let dfd = frame.arg1() as i32;
    let mode = frame.arg3() as i32;
    let _flags = frame.arg4() as i32;

    if !(0..=7).contains(&mode) {
        return Err(SyscallError::EINVAL);
    }

    let path = UserCStr::from_u64(frame.arg2()).to_string(256)?;
    let full_path = resolve_at_path(dfd, &path)?;
    let _dentry = crate::fs::resolve_path(&full_path)?;

    Ok(0)
}
