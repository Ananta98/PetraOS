//! sys_statfs system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult, UserPtr, UserCStr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::File;
use crate::fs::vfs::types::{InodeType, LinuxStat, SeekWhence, Stat, StatFs};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;


/// `sys_statfs` (SYS_STATFS = 137)
/// Get filesystem statistics by pathname.
pub fn sys_statfs(frame: &mut SyscallFrame) -> SyscallResult {
    let path = UserCStr::from_u64(frame.arg1()).to_string(256)?;
    let buf_ptr = UserPtr::<StatFs>::from_u64(frame.arg2());

    let full_path = resolve_at_path(AT_FDCWD, &path)?;
    let _dentry = crate::fs::resolve_path(&full_path)?;

    let statfs = make_statfs();
    buf_ptr.write(statfs).ok_or(SyscallError::EFAULT)?;
    Ok(0)
}
