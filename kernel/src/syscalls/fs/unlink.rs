//! System calls for removing names from the filesystem (`unlink`, `unlinkat`).

use super::*;
use crate::syscalls::{wrap_syscall, SyscallResult, UserCStr};

/// `sys_unlink` (SYS_UNLINK = 87)
/// Delete a name and possibly the file it refers to.
#[wrap_syscall]
pub fn sys_unlink(path_ptr: UserCStr) -> SyscallResult {
    let path = path_ptr.to_string(256)?;
    let full_path = resolve_at_path(AT_FDCWD, &path)?;
    crate::fs::unlink(&full_path)?;
    Ok(0)
}

/// `sys_unlinkat` (SYS_UNLINKAT = 263)
/// Delete a name relative to a directory file descriptor.
#[wrap_syscall]
pub fn sys_unlinkat(dfd: i32, path_ptr: UserCStr, flags: i32) -> SyscallResult {
    let path = path_ptr.to_string(256)?;
    let full_path = resolve_at_path(dfd, &path)?;
    if (flags & crate::fs::AT_REMOVEDIR) != 0 {
        crate::fs::rmdir(&full_path)?;
    } else {
        crate::fs::unlink(&full_path)?;
    }
    Ok(0)
}
