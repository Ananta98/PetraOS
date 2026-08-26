//! sys_fstatfs system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult, UserPtr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::File;
use crate::fs::vfs::types::{InodeType, LinuxStat, SeekWhence, Stat, StatFs};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;


/// `sys_fstatfs` (SYS_FSTATFS = 138)
/// Get filesystem statistics by open file descriptor.
pub fn sys_fstatfs(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let buf_ptr = UserPtr::<StatFs>::from_u64(frame.arg2());

    if fd < 0 {
        return Err(SyscallError::EBADF);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let _file = proc.fd_table.get(fd)?;
    drop(proc);

    let statfs = make_statfs();
    buf_ptr.write(statfs).ok_or(SyscallError::EFAULT)?;
    Ok(0)
}
