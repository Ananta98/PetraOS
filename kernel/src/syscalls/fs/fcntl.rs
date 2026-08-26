//! sys_fcntl system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::File;
use crate::fs::vfs::types::{InodeType, LinuxStat, SeekWhence, Stat, StatFs};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;


/// `sys_fcntl` (SYS_FCNTL = 72)
/// Manipulate file descriptor properties.
pub fn sys_fcntl(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let cmd = frame.arg2() as i32;
    let arg = frame.arg3();

    if fd < 0 {
        return Err(SyscallError::EBADF);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();

    match cmd {
        F_DUPFD => {
            let min_fd = arg as i32;
            if min_fd < 0 {
                return Err(SyscallError::EINVAL);
            }
            let file = proc.fd_table.get(fd)?;
            let new_fd = proc.fd_table.alloc_from(min_fd, file, 0)?;
            Ok(new_fd as usize)
        }
        F_DUPFD_CLOEXEC => {
            let min_fd = arg as i32;
            if min_fd < 0 {
                return Err(SyscallError::EINVAL);
            }
            let file = proc.fd_table.get(fd)?;
            let new_fd = proc
                .fd_table
                .alloc_from(min_fd, file, crate::fs::fd::FD_CLOEXEC)?;
            Ok(new_fd as usize)
        }
        F_GETFD => {
            let flags = proc.fd_table.get_flags(fd)?;
            Ok(flags as usize)
        }
        F_SETFD => {
            let flags = arg as u32;
            proc.fd_table.set_flags(fd, flags)?;
            Ok(0)
        }
        F_GETFL => {
            let file = proc.fd_table.get(fd)?;
            Ok(file.flags() as usize)
        }
        F_SETFL => {
            let flags = arg as u32;
            let file = proc.fd_table.get(fd)?;
            file.set_flags(flags);
            Ok(0)
        }
        F_GETLK | F_SETLK | F_SETLKW | F_SETOWN | F_GETOWN => Ok(0),
        _ => Ok(0),
    }
}
