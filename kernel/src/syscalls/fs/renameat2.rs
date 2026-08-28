//! sys_renameat2 system call handler.

use super::*;
use crate::syscalls::{SyscallResult, UserCStr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::File;
use crate::fs::vfs::types::{InodeType, LinuxStat, SeekWhence, Stat, StatFs};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;


pub const RENAME_NOREPLACE: u32 = 1;
pub const RENAME_EXCHANGE: u32 = 2;
pub const RENAME_WHITEOUT: u32 = 4;

/// `sys_renameat2` (SYS_RENAMEAT2 = 316)
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
