//! System calls for renaming files (`rename`, `renameat`, `renameat2`).

use super::*;
use crate::syscalls::{wrap_syscall, SyscallError, SyscallResult, UserCStr};

pub const RENAME_NOREPLACE: u32 = 1;
pub const RENAME_EXCHANGE: u32 = 2;
pub const RENAME_WHITEOUT: u32 = 4;

/// `sys_rename` (SYS_RENAME = 82)
/// Change the name or location of a file.
#[wrap_syscall]
pub fn sys_rename(old_path_ptr: UserCStr, new_path_ptr: UserCStr) -> SyscallResult {
    let old_path = old_path_ptr.to_string(256)?;
    let new_path = new_path_ptr.to_string(256)?;
    let old_full = resolve_at_path(AT_FDCWD, &old_path)?;
    let new_full = resolve_at_path(AT_FDCWD, &new_path)?;
    crate::fs::rename(&old_full, &new_full)?;
    Ok(0)
}

/// `sys_renameat` (SYS_RENAMEAT = 264)
/// Rename a file relative to directory file descriptors.
#[wrap_syscall]
pub fn sys_renameat(
    olddfd: i32,
    old_path_ptr: UserCStr,
    newdfd: i32,
    new_path_ptr: UserCStr,
) -> SyscallResult {
    let old_path = old_path_ptr.to_string(256)?;
    let new_path = new_path_ptr.to_string(256)?;
    let old_full = resolve_at_path(olddfd, &old_path)?;
    let new_full = resolve_at_path(newdfd, &new_path)?;
    crate::fs::rename(&old_full, &new_full)?;
    Ok(0)
}

/// `sys_renameat2` (SYS_RENAMEAT2 = 316)
/// Rename a file relative to directory file descriptors with flags.
#[wrap_syscall]
pub fn sys_renameat2(
    olddfd: i32,
    old_path_ptr: UserCStr,
    newdfd: i32,
    new_path_ptr: UserCStr,
    flags: u32,
) -> SyscallResult {
    if (flags & !(RENAME_NOREPLACE | RENAME_EXCHANGE | RENAME_WHITEOUT)) != 0 {
        return Err(SyscallError::EINVAL);
    }
    if (flags & (RENAME_EXCHANGE | RENAME_WHITEOUT)) != 0 {
        return Err(SyscallError::EINVAL);
    }

    let old_path = old_path_ptr.to_string(256)?;
    let new_path = new_path_ptr.to_string(256)?;
    let old_full = resolve_at_path(olddfd, &old_path)?;
    let new_full = resolve_at_path(newdfd, &new_path)?;

    if (flags & RENAME_NOREPLACE) != 0 {
        if crate::fs::resolve_path(&new_full).is_ok() {
            return Err(SyscallError::EEXIST);
        }
    }

    crate::fs::rename(&old_full, &new_full)?;
    Ok(0)
}
