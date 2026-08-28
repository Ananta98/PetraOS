//! System calls for creating special or generic filesystem nodes (`mknodat`).

use super::*;
use crate::arch::syscall::syscall::SyscallFrame;
use crate::syscalls::{SyscallResult, UserCStr};

/// `sys_mknodat` (SYS_MKNODAT = 259)
/// Create a special or ordinary file relative to a directory file descriptor.
pub fn sys_mknodat(frame: &mut SyscallFrame) -> SyscallResult {
    let dfd = frame.arg1() as i32;
    let _mode = frame.arg3() as u32;
    let _dev = frame.arg4() as u64;

    let path = UserCStr::from_u64(frame.arg2()).to_string(256)?;
    let full_path = resolve_at_path(dfd, &path)?;
    let _ = crate::fs::create_file(&full_path)?;
    Ok(0)
}
