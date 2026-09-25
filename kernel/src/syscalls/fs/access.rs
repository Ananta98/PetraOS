//! System calls for checking file accessibility and permissions (`access`, `faccessat`).

use super::*;
use crate::fs::vfs::perm::{check_access_stat, AT_EACCESS};
use crate::syscalls::{wrap_syscall, SyscallError, SyscallResult, UserCStr};

/// `sys_access` (SYS_ACCESS = 21)
/// Check real-uid/real-gid permissions for a file.
#[wrap_syscall]
pub fn sys_access(path_ptr: UserCStr, mode: u32) -> SyscallResult {
    if mode & !0x7 != 0 {
        return Err(SyscallError::EINVAL);
    }

    let path = path_ptr.to_string(4096)?;
    let full_path = resolve_at_path(AT_FDCWD, &path)?;
    let st = crate::fs::stat(&full_path)?;

    check_access_stat(&st, mode, false)?;
    Ok(0)
}

/// `sys_faccessat` (SYS_FACCESSAT = 269)
/// Check permissions relative to a directory fd; honors `AT_EACCESS`.
#[wrap_syscall]
pub fn sys_faccessat(dfd: i32, path_ptr: UserCStr, mode: u32, flags: i32) -> SyscallResult {
    if mode & !0x7 != 0 {
        return Err(SyscallError::EINVAL);
    }

    let path = path_ptr.to_string(4096)?;
    let full_path = resolve_at_path(dfd, &path)?;
    let st = crate::fs::stat(&full_path)?;

    let use_effective = (flags & AT_EACCESS) != 0;
    check_access_stat(&st, mode, use_effective)?;
    Ok(0)
}
