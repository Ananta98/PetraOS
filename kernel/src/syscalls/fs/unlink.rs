//! System calls for removing names from the filesystem (`unlink`, `unlinkat`).

use super::*;
use crate::arch::syscall::syscall::SyscallFrame;
use crate::syscalls::{SyscallResult, UserCStr};

/// `sys_unlink` (SYS_UNLINK = 87)
/// Delete a name and possibly the file it refers to.
pub fn sys_unlink(frame: &mut SyscallFrame) -> SyscallResult {
    let path = UserCStr::from_u64(frame.arg1()).to_string(256)?;
    let full_path = resolve_at_path(AT_FDCWD, &path)?;
    crate::fs::unlink(&full_path)?;
    Ok(0)
}

/// `sys_unlinkat` (SYS_UNLINKAT = 263)
/// Delete a name relative to a directory file descriptor.
pub fn sys_unlinkat(frame: &mut SyscallFrame) -> SyscallResult {
    let dfd = frame.arg1() as i32;
    let flags = frame.arg3() as i32;

    let path = UserCStr::from_u64(frame.arg2()).to_string(256)?;
    let full_path = resolve_at_path(dfd, &path)?;
    if (flags & crate::fs::AT_REMOVEDIR) != 0 {
        crate::fs::rmdir(&full_path)?;
    } else {
        crate::fs::unlink(&full_path)?;
    }
    Ok(0)
}
