// SPDX-License-Identifier: GPL-2.0

//! The `open` and `openat` system calls (Syscalls #2 and #257 on x86_64).

use ostd::arch::cpu::context::UserContext;

use crate::{
    api::errno::{Errno, Result},
    fs::vfs::Mode,
    proc::thread::Thread,
    syscall::fs::utils::{AT_FDCWD, parse_open_flags, read_user_string, resolve_at_start},
};

/// System call `openat(int dirfd, const char *pathname, int flags, mode_t mode)` (Syscall #257).
pub fn sys_openat(
    dirfd: i32,
    path_ptr: usize,
    flags: i32,
    mode: u32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();
    let path = read_user_string(&vmar, path_ptr, 4096)?;

    let start = resolve_at_start(&proc, dirfd, &path)?;
    let (open_flags, fd_flags) = parse_open_flags(flags);

    let file = crate::fs::vfs::open_at(start, &path, open_flags, Mode::from_bits_truncate(mode))?;
    let fd = proc.fd_table.lock().alloc_fd(file, fd_flags)?;

    Ok(fd as usize)
}

/// System call `open(const char *pathname, int flags, mode_t mode)` (Syscall #2).
pub fn sys_open(
    path_ptr: usize,
    flags: i32,
    mode: u32,
    ctx: &mut UserContext,
) -> Result<usize> {
    sys_openat(AT_FDCWD, path_ptr, flags, mode, ctx)
}
