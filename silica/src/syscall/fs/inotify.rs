// SPDX-License-Identifier: GPL-2.0

//! Inotify system calls (`inotify_init`, `inotify_init1`, `inotify_add_watch`, `inotify_rm_watch`).

use ostd::arch::cpu::context::UserContext;

use crate::{
    fs::vfs::LookupFlags,
    proc::thread::Thread,
    syscall::fs::utils::{AT_FDCWD, UserCStr, resolve_at_start},
    uapi::{
        errno::{Errno, Result},
        inotify::IN_INIT_FLAGS_MASK,
    },
};

/// System call `inotify_init1(int flags)` (Syscall #294).
pub fn sys_inotify_init1(flags: i32, _ctx: &mut UserContext) -> Result<usize> {
    if (flags & !IN_INIT_FLAGS_MASK) != 0 {
        return Err(Errno::EINVAL);
    }
    let proc = Thread::proc_or_esrch()?;
    let (file, fd_flags) = crate::fs::fd::create_inotify_file(flags)?;
    let fd = proc.fd_table.lock().alloc_fd(file, fd_flags)?;
    Ok(fd as usize)
}

/// System call `inotify_init(void)` (Syscall #253).
pub fn sys_inotify_init(ctx: &mut UserContext) -> Result<usize> {
    sys_inotify_init1(0, ctx)
}

/// System call `inotify_add_watch(int fd, const char *pathname, uint32_t mask)` (Syscall #254).
pub fn sys_inotify_add_watch(
    fd: i32,
    pathname: usize,
    mask: u32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if pathname == 0 {
        return Err(Errno::EFAULT);
    }
    let proc = Thread::proc_or_esrch()?;
    let file = proc.fd_table.lock().get(fd)?;
    let inotify = file.as_inotify().ok_or(Errno::EBADF)?;

    let vmar = proc.vmspace();
    let path = UserCStr::new(pathname).read_string(&vmar, 4096)?;
    if path.is_empty() {
        return Err(Errno::ENOENT);
    }

    let start = resolve_at_start(&proc, AT_FDCWD, &path)?;
    let _ = crate::fs::vfs::lookup_at(&start, &path, LookupFlags::empty())?;

    let wd = inotify.add_watch(&path, mask)?;
    Ok(wd as usize)
}

/// System call `inotify_rm_watch(int fd, int wd)` (Syscall #255).
pub fn sys_inotify_rm_watch(fd: i32, wd: i32, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let file = proc.fd_table.lock().get(fd)?;
    let inotify = file.as_inotify().ok_or(Errno::EBADF)?;

    inotify.rm_watch(wd)?;
    Ok(0)
}
