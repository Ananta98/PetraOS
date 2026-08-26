//! sys_symlinkat system call handler.

use super::*;
use crate::syscalls::{SyscallResult, UserCStr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::File;
use crate::fs::vfs::types::{InodeType, LinuxStat, SeekWhence, Stat, StatFs};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;


/// `sys_symlinkat` (SYS_SYMLINKAT = 266)
pub fn sys_symlinkat(frame: &mut SyscallFrame) -> SyscallResult {
    let newdfd = frame.arg2() as i32;

    let target = UserCStr::from_u64(frame.arg1()).to_string(256)?;
    let link_path = UserCStr::from_u64(frame.arg3()).to_string(256)?;
    let full_path = resolve_at_path(newdfd, &link_path)?;
    crate::fs::symlink(&full_path, &target)?;
    Ok(0)
}
