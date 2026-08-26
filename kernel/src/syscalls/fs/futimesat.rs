//! sys_futimesat system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult, UserPtr, UserCStr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::File;
use crate::fs::vfs::types::{InodeType, LinuxStat, SeekWhence, Stat, StatFs};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;


/// `sys_futimesat` (SYS_FUTIMESAT = 261)
pub fn sys_futimesat(frame: &mut SyscallFrame) -> SyscallResult {
    let dfd = frame.arg1() as i32;
    let path_cstr = UserCStr::from_u64(frame.arg2());
    let utimes_ptr = UserPtr::<LinuxTimespec>::from_u64(frame.arg3());

    if path_cstr.is_null() {
        return Ok(0);
    }
    let path = path_cstr.to_string(256)?;
    let full_path = resolve_at_path(dfd, &path)?;
    let (atime, mtime) = if !utimes_ptr.is_null() {
        let times = utimes_ptr.as_slice(2).ok_or(SyscallError::EFAULT)?;
        (
            resolve_timespec(times[0], 0)?,
            resolve_timespec(times[1], 0)?,
        )
    } else {
        // POSIX: a null timeval selects the current time for both fields.
        let now = wall_now_secs();
        (now, now)
    };
    crate::fs::utimens(&full_path, atime, mtime)?;
    Ok(0)
}
