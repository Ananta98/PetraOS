//! sys_ppoll system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult, UserPtr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::File;
use crate::fs::vfs::types::{InodeType, LinuxStat, SeekWhence, Stat, StatFs};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;


/// `sys_ppoll` (SYS_PPOLL = 271)
pub fn sys_ppoll(frame: &mut SyscallFrame) -> SyscallResult {
    let fds_ptr = UserPtr::<PollFd>::from_u64(frame.arg1());
    let nfds = frame.arg2() as usize;
    let ts_ptr = UserPtr::<crate::syscalls::time::TimeSpec>::from_u64(frame.arg3());

    let timeout_ms = if ts_ptr.is_null() {
        -1
    } else {
        let ts = ts_ptr.read().ok_or(SyscallError::EFAULT)?;
        if ts.tv_sec < 0 || ts.tv_nsec < 0 {
            return Err(SyscallError::EINVAL);
        }
        (ts.tv_sec * 1000 + ts.tv_nsec / 1_000_000) as i32
    };

    do_poll(fds_ptr, nfds, timeout_ms)
}
