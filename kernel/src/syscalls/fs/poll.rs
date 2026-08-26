//! sys_poll system call handler.

use super::*;
use crate::syscalls::{SyscallResult, UserPtr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::File;
use crate::fs::vfs::types::{InodeType, LinuxStat, SeekWhence, Stat, StatFs};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;


/// `sys_poll` (SYS_POLL = 7)
pub fn sys_poll(frame: &mut SyscallFrame) -> SyscallResult {
    let fds_ptr = UserPtr::<PollFd>::from_u64(frame.arg1());
    let nfds = frame.arg2() as usize;
    let timeout_ms = frame.arg3() as i32;

    do_poll(fds_ptr, nfds, timeout_ms)
}
