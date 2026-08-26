//! sys_renameat system call handler.

use super::*;
use crate::syscalls::{SyscallResult, UserCStr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::File;
use crate::fs::vfs::types::{InodeType, LinuxStat, SeekWhence, Stat, StatFs};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;


/// `sys_renameat` (SYS_RENAMEAT = 264)
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
