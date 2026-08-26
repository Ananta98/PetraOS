//! sys_symlink system call handler.

use super::*;
use crate::syscalls::{SyscallResult, UserCStr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::File;
use crate::fs::vfs::types::{InodeType, LinuxStat, SeekWhence, Stat, StatFs};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;


/// `sys_symlink` (SYS_SYMLINK = 88)
pub fn sys_symlink(frame: &mut SyscallFrame) -> SyscallResult {
    let target = UserCStr::from_u64(frame.arg1()).to_string(256)?;
    let link_path = UserCStr::from_u64(frame.arg2()).to_string(256)?;
    let full_path = resolve_at_path(AT_FDCWD, &link_path)?;
    crate::fs::symlink(&full_path, &target)?;
    Ok(0)
}
