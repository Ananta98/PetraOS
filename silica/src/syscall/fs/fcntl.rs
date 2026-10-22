// SPDX-License-Identifier: GPL-2.0

//! System calls for file descriptor manipulation and locking: `fcntl`, `flock`.

use ostd::arch::cpu::context::UserContext;

use crate::{
    uapi::errno::{Errno, Result},
    fs::{fd::FdFlags, vfs::OpenFlags},
    proc::thread::Thread,
};

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

/// System call `fcntl(int fd, int cmd, ...)` (Syscall #72).
pub fn sys_fcntl(fd: i32, cmd: i32, arg: usize, _ctx: &mut UserContext) -> Result<usize> {
    if fd < 0 {
        return Err(Errno::EBADF);
    }

    let proc = Thread::proc_or_esrch()?;

    match cmd {
        F_DUPFD => {
            let min_fd = arg as i32;
            if min_fd < 0 {
                return Err(Errno::EINVAL);
            }
            let mut table = proc.fd_table.lock();
            let file = table.get(fd)?;
            let new_fd = table.alloc_fd_at(min_fd, file, FdFlags::empty())?;
            Ok(new_fd as usize)
        }
        F_DUPFD_CLOEXEC => {
            let min_fd = arg as i32;
            if min_fd < 0 {
                return Err(Errno::EINVAL);
            }
            let mut table = proc.fd_table.lock();
            let file = table.get(fd)?;
            let new_fd = table.alloc_fd_at(min_fd, file, FdFlags::FD_CLOEXEC)?;
            Ok(new_fd as usize)
        }
        F_GETFD => {
            let table = proc.fd_table.lock();
            let flags = table.get_flags(fd)?;
            let res = if flags.contains(FdFlags::FD_CLOEXEC) {
                1
            } else {
                0
            };
            Ok(res)
        }
        F_SETFD => {
            let mut table = proc.fd_table.lock();
            let cloexec = (arg & 1) != 0;
            let flags = if cloexec {
                FdFlags::FD_CLOEXEC
            } else {
                FdFlags::empty()
            };
            table.set_flags(fd, flags)?;
            Ok(0)
        }
        F_GETFL => {
            let table = proc.fd_table.lock();
            let file = table.get(fd)?;
            let flags = *file.flags.lock();
            let mut res = 0usize;
            let is_r = flags.contains(OpenFlags::READ);
            let is_w = flags.contains(OpenFlags::WRITE);
            if is_r && is_w {
                res |= 2; // O_RDWR
            } else if is_w {
                res |= 1; // O_WRONLY
            } else {
                res |= 0; // O_RDONLY
            }
            if flags.contains(OpenFlags::APPEND) {
                res |= 0o2000;
            }
            if flags.contains(OpenFlags::NONBLOCK) {
                res |= 0o4000;
            }
            Ok(res)
        }
        F_SETFL => {
            let table = proc.fd_table.lock();
            let file = table.get(fd)?;
            let arg_u32 = arg as u32;
            let mut flags = file.flags.lock();
            if (arg_u32 & 0o2000) != 0 {
                *flags |= OpenFlags::APPEND;
            } else {
                *flags &= !OpenFlags::APPEND;
            }
            let nonblocking = (arg_u32 & 0o4000) != 0;
            if nonblocking {
                *flags |= OpenFlags::NONBLOCK;
            } else {
                *flags &= !OpenFlags::NONBLOCK;
            }
            if let Some(sock) = file.as_socket() {
                *sock.nonblocking.lock() = nonblocking;
            }
            Ok(0)
        }
        F_GETLK | F_SETLK | F_SETLKW | F_SETOWN | F_GETOWN => {
            let table = proc.fd_table.lock();
            let _file = table.get(fd)?;
            Ok(0)
        }
        _ => Err(Errno::EINVAL),
    }
}

/// System call `flock(int fd, int operation)` (Syscall #73).
pub fn sys_flock(fd: i32, _operation: i32, _ctx: &mut UserContext) -> Result<usize> {
    if fd < 0 {
        return Err(Errno::EBADF);
    }
    let proc = Thread::proc_or_esrch()?;
    let _file = proc.file(fd)?;
    Ok(0)
}
