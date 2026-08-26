//! sys_rename system call handler.

use super::*;
use crate::syscalls::{SyscallResult, UserCStr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::File;
use crate::fs::vfs::types::{InodeType, LinuxStat, SeekWhence, Stat, StatFs};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;


/// `sys_rename` (SYS_RENAME = 82)
pub fn sys_rename(frame: &mut SyscallFrame) -> SyscallResult {
    let old_path = UserCStr::from_u64(frame.arg1()).to_string(256)?;
    let new_path = UserCStr::from_u64(frame.arg2()).to_string(256)?;
    let old_full = resolve_at_path(AT_FDCWD, &old_path)?;
    let new_full = resolve_at_path(AT_FDCWD, &new_path)?;
    crate::fs::rename(&old_full, &new_full)?;
    Ok(0)
}
