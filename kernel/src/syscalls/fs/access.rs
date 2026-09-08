//! System calls for checking file accessibility and permissions (`access`, `faccessat`).

use super::*;
use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::vfs::perm::{AT_EACCESS, check_access_stat};
use crate::syscalls::{SyscallError, SyscallResult, UserCStr};

/// `sys_access` (SYS_ACCESS = 21)
/// Check real-uid/real-gid permissions for a file.
pub fn sys_access(frame: &mut SyscallFrame) -> SyscallResult {
    let mode = frame.arg2() as u32;

    if mode & !0x7 != 0 {
        return Err(SyscallError::EINVAL);
    }

    let path = UserCStr::from_u64(frame.arg1()).to_string(256)?;
    let full_path = resolve_at_path(AT_FDCWD, &path)?;
    let st = crate::fs::stat(&full_path)?;

    check_access_stat(&st, mode, false)?;
    Ok(0)
}

/// `sys_faccessat` (SYS_FACCESSAT = 269)
/// Check permissions relative to a directory fd; honors `AT_EACCESS`.
pub fn sys_faccessat(frame: &mut SyscallFrame) -> SyscallResult {
    let dfd = frame.arg1() as i32;
    let mode = frame.arg3() as u32;
    let flags = frame.arg4() as i32;

    if mode & !0x7 != 0 {
        return Err(SyscallError::EINVAL);
    }

    let path = UserCStr::from_u64(frame.arg2()).to_string(256)?;
    let full_path = resolve_at_path(dfd, &path)?;
    let st = crate::fs::stat(&full_path)?;

    let use_effective = (flags & AT_EACCESS) != 0;
    check_access_stat(&st, mode, use_effective)?;
    Ok(0)
}
