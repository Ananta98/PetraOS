//! System calls for file control and locking (`fcntl`, `flock`).

use crate::arch::syscall::syscall::SyscallFrame;
use crate::syscalls::{SyscallError, SyscallResult};

pub const F_DUPFD: i32 = 0;
pub const F_GETFD: i32 = 1;
pub const F_SETFD: i32 = 2;
pub const F_GETFL: i32 = 3;
pub const F_SETFL: i32 = 4;
pub const F_GETLK: i32 = 5;
pub const F_SETLK: i32 = 6;
pub const F_SETLKW: i32 = 7;
pub const F_SETOWN: i32 = 8;
pub const F_GETOWN: i32 = 9;
pub const F_DUPFD_CLOEXEC: i32 = 1030;

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

/// `sys_flock` (SYS_FLOCK = 73)
/// Apply or remove an advisory lock on an open file.
pub fn sys_flock(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let _operation = frame.arg2() as i32;

    if fd < 0 {
        return Err(SyscallError::EBADF);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let _file = proc.fd_table.get(fd)?;
    drop(proc);

    // Advisory file locking
    Ok(0)
}
