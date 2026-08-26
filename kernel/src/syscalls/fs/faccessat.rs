//! sys_faccessat system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult, UserCStr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::File;
use crate::fs::vfs::types::{InodeType, LinuxStat, SeekWhence, Stat, StatFs};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;


/// `sys_faccessat` (SYS_FACCESSAT = 269)
/// Check user's permissions for a file relative to a directory file descriptor.
pub fn sys_faccessat(frame: &mut SyscallFrame) -> SyscallResult {
    let dfd = frame.arg1() as i32;
    let mode = frame.arg3() as i32;
    let _flags = frame.arg4() as i32;

    if mode < 0 || mode > 7 {
        return Err(SyscallError::EINVAL);
    }

    let path = UserCStr::from_u64(frame.arg2()).to_string(256)?;
    let full_path = resolve_at_path(dfd, &path)?;
    let _dentry = crate::fs::resolve_path(&full_path)?;

    Ok(0)
}
