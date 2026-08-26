//! sys_fchmodat system call handler.

use super::*;
use crate::syscalls::{SyscallResult, UserCStr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::File;
use crate::fs::vfs::types::{InodeType, LinuxStat, SeekWhence, Stat, StatFs};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;


/// `sys_fchmodat` (SYS_FCHMODAT = 268)
pub fn sys_fchmodat(frame: &mut SyscallFrame) -> SyscallResult {
    let dfd = frame.arg1() as i32;
    let mode = frame.arg3() as u32;
    let _flags = frame.arg4() as i32;

    let path = UserCStr::from_u64(frame.arg2()).to_string(256)?;
    let full_path = resolve_at_path(dfd, &path)?;
    crate::fs::chmod(&full_path, mode)?;
    Ok(0)
}
