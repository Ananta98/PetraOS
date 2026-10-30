// SPDX-License-Identifier: GPL-2.0

//! System call `fadvise64` (Syscall #221 on x86_64).

use ostd::arch::cpu::context::UserContext;

use crate::{
    uapi::errno::{Errno, Result},
    proc::thread::Thread,
};

/// System call `fadvise64(int fd, off_t offset, off_t len, int advice)` (Syscall #221).
pub fn sys_fadvise64(
    fd: i32,
    _offset: i64,
    len: i64,
    advice: i32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if fd < 0 {
        return Err(Errno::EBADF);
    }
    if len < 0 {
        return Err(Errno::EINVAL);
    }
    // POSIX advice values (POSIX_FADV_NORMAL=0..POSIX_FADV_NOREUSE=5)
    if !(0..=5).contains(&advice) {
        return Err(Errno::EINVAL);
    }
    let proc = Thread::proc_or_esrch()?;
    let _file = proc.file(fd)?;
    Ok(0)
}

/// System call `readahead(int fd, off64_t offset, size_t count)` (Syscall #187).
pub fn sys_readahead(
    fd: i32,
    offset: i64,
    _count: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if fd < 0 {
        return Err(Errno::EBADF);
    }
    if offset < 0 {
        return Err(Errno::EINVAL);
    }
    let proc = Thread::proc_or_esrch()?;
    let file = proc.file(fd)?;
    if !file.inode.is_file() {
        return Err(Errno::EINVAL);
    }
    Ok(0)
}

/// System call `sync_file_range(int fd, off64_t offset, off64_t nbytes, unsigned int flags)` (Syscall #277).
pub fn sys_sync_file_range(
    fd: i32,
    offset: i64,
    nbytes: i64,
    flags: u32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if fd < 0 {
        return Err(Errno::EBADF);
    }
    if offset < 0 || nbytes < 0 {
        return Err(Errno::EINVAL);
    }
    if (flags & !7) != 0 {
        return Err(Errno::EINVAL);
    }
    let proc = Thread::proc_or_esrch()?;
    let file = proc.file(fd)?;
    if (flags & 2) != 0 {
        let _ = file.fsync(true);
    }
    Ok(0)
}
