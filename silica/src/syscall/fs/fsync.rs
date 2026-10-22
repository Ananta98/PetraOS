// SPDX-License-Identifier: GPL-2.0

//! System calls for synchronizing file data with storage: `fsync`, `fdatasync`.

use ostd::arch::cpu::context::UserContext;

use crate::{
    api::errno::{Errno, Result},
    proc::thread::Thread,
};

/// System call `fsync(int fd)` (Syscall #74).
pub fn sys_fsync(fd: i32, _ctx: &mut UserContext) -> Result<usize> {
    if fd < 0 {
        return Err(Errno::EBADF);
    }
    let proc = Thread::proc_or_esrch()?;
    let file = proc.file(fd)?;
    file.fsync(false)?;
    Ok(0)
}

/// System call `fdatasync(int fd)` (Syscall #75).
pub fn sys_fdatasync(fd: i32, _ctx: &mut UserContext) -> Result<usize> {
    if fd < 0 {
        return Err(Errno::EBADF);
    }
    let proc = Thread::proc_or_esrch()?;
    let file = proc.file(fd)?;
    file.fsync(true)?;
    Ok(0)
}

/// System call `sync(void)` (Syscall #162).
pub fn sys_sync(_ctx: &mut UserContext) -> Result<usize> {
    crate::fs::vfs::mount::sync_all()?;
    Ok(0)
}

/// System call `syncfs(int fd)` (Syscall #306).
pub fn sys_syncfs(fd: i32, _ctx: &mut UserContext) -> Result<usize> {
    if fd < 0 {
        return Err(Errno::EBADF);
    }
    let proc = Thread::proc_or_esrch()?;
    let file = proc.file(fd)?;
    file.fsync(false)?;
    if let Some(ref pn) = file.path_node {
        let _ = pn.mount.fs.sync();
    }
    Ok(0)
}
