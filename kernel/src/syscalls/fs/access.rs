//! sys_access system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult, UserCStr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::File;
use crate::fs::vfs::types::{InodeType, LinuxStat, SeekWhence, Stat, StatFs};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;


/// `sys_access` (SYS_ACCESS = 21)
/// Check user's permissions for a file.
pub fn sys_access(frame: &mut SyscallFrame) -> SyscallResult {
    let mode = frame.arg2() as i32;

    if mode < 0 || mode > 7 {
        return Err(SyscallError::EINVAL);
    }

    let path = UserCStr::from_u64(frame.arg1()).to_string(256)?;
    let full_path = resolve_at_path(AT_FDCWD, &path)?;
    let _dentry = crate::fs::resolve_path(&full_path)?;

    Ok(0)
}
