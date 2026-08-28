//! System calls for renaming files (`rename`, `renameat`, `renameat2`).

use super::*;
use crate::arch::syscall::syscall::SyscallFrame;
use crate::syscalls::{SyscallError, SyscallResult, UserCStr};

pub const RENAME_NOREPLACE: u32 = 1;
pub const RENAME_EXCHANGE: u32 = 2;
pub const RENAME_WHITEOUT: u32 = 4;

/// `sys_rename` (SYS_RENAME = 82)
/// Change the name or location of a file.
pub fn sys_rename(frame: &mut SyscallFrame) -> SyscallResult {
    let old_path = UserCStr::from_u64(frame.arg1()).to_string(256)?;
    let new_path = UserCStr::from_u64(frame.arg2()).to_string(256)?;
    let old_full = resolve_at_path(AT_FDCWD, &old_path)?;
    let new_full = resolve_at_path(AT_FDCWD, &new_path)?;
    crate::fs::rename(&old_full, &new_full)?;
    Ok(0)
}

/// `sys_renameat` (SYS_RENAMEAT = 264)
/// Rename a file relative to directory file descriptors.
pub fn sys_renameat(frame: &mut SyscallFrame) -> SyscallResult {
    let olddfd = frame.arg1() as i32;
    let newdfd = frame.arg3() as i32;

    let old_path = UserCStr::from_u64(frame.arg2()).to_string(256)?;
    let new_path = UserCStr::from_u64(frame.arg4()).to_string(256)?;
    let old_full = resolve_at_path(olddfd, &old_path)?;
    let new_full = resolve_at_path(newdfd, &new_path)?;
    crate::fs::rename(&old_full, &new_full)?;
    Ok(0)
}

/// `sys_renameat2` (SYS_RENAMEAT2 = 316)
/// Rename a file relative to directory file descriptors with flags.
pub fn sys_renameat2(frame: &mut SyscallFrame) -> SyscallResult {
    let olddfd = frame.arg1() as i32;
    let newdfd = frame.arg3() as i32;
    let flags = frame.arg5() as u32;

    if (flags & !(RENAME_NOREPLACE | RENAME_EXCHANGE | RENAME_WHITEOUT)) != 0 {
        return Err(SyscallError::EINVAL);
    }
    if (flags & (RENAME_EXCHANGE | RENAME_WHITEOUT)) != 0 {
        return Err(SyscallError::EINVAL);
    }

    let old_path = UserCStr::from_u64(frame.arg2()).to_string(256)?;
    let new_path = UserCStr::from_u64(frame.arg4()).to_string(256)?;
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
