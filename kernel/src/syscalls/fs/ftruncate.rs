//! sys_ftruncate system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::File;
use crate::fs::vfs::types::{InodeType, LinuxStat, SeekWhence, Stat, StatFs};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;


/// `sys_ftruncate` (SYS_FTRUNCATE = 77)
pub fn sys_ftruncate(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let length = frame.arg2() as usize;

    if fd < 0 {
        return Err(SyscallError::EBADF);
    }
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let file = proc.fd_table.get(fd)?;
    drop(proc);

    file.ops
        .truncate(length)
        .or_else(|_| file.dentry.inode.ops.truncate(length))?;
    Ok(0)
}
