//! sys_lstat system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult, UserPtr, UserCStr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::File;
use crate::fs::vfs::types::{InodeType, LinuxStat, SeekWhence, Stat, StatFs};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;


/// `sys_lstat` (SYS_LSTAT = 6)
/// Get file status without following symlinks.
pub fn sys_lstat(frame: &mut SyscallFrame) -> SyscallResult {
    let path_ptr = UserCStr::from_u64(frame.arg1());
    let statbuf = UserPtr::<LinuxStat>::from_u64(frame.arg2());

    let path = path_ptr.to_string(256)?;
    let full_path = resolve_at_path(AT_FDCWD, &path)?;
    let vfs_stat = crate::fs::lstat(&full_path)?;

    let linux_stat = copy_to_linux_stat(&vfs_stat);
    statbuf.write(linux_stat).ok_or(SyscallError::EFAULT)?;

    Ok(0)
}
