//! sys_openat system call handler.

use super::*;
use crate::syscalls::{SyscallResult, UserCStr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::File;
use crate::fs::vfs::types::{InodeType, LinuxStat, SeekWhence, Stat, StatFs};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;


/// `sys_openat` (SYS_OPENAT = 257)
/// Open a file relative to directory descriptor.
pub fn sys_openat(frame: &mut SyscallFrame) -> SyscallResult {
    let dfd = frame.arg1() as i32;
    let path_ptr = UserCStr::from_u64(frame.arg2());
    let flags = frame.arg3() as u32;

    let path = path_ptr.to_string(256)?;
    do_openat(dfd, &path, flags)
}
