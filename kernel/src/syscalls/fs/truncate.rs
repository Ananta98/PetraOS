//! sys_truncate system call handler.

use super::*;
use crate::syscalls::{SyscallResult, UserCStr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::File;
use crate::fs::vfs::types::{InodeType, LinuxStat, SeekWhence, Stat, StatFs};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;


/// `sys_truncate` (SYS_TRUNCATE = 76)
pub fn sys_truncate(frame: &mut SyscallFrame) -> SyscallResult {
    let length = frame.arg2() as usize;

    let path = UserCStr::from_u64(frame.arg1()).to_string(256)?;
    let full_path = resolve_at_path(AT_FDCWD, &path)?;
    crate::fs::truncate(&full_path, length)?;
    Ok(0)
}
