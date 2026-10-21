// SPDX-License-Identifier: GPL-2.0

//! Filesystem manipulation and namespace operations system calls:
//! `truncate`, `ftruncate`, `rename`, `mkdir`, `rmdir`, `link`, `unlink`, `symlink`, `readlink`,
//! and their relative `*at` counterparts.

use ostd::arch::cpu::context::UserContext;

use crate::{
    api::errno::{Errno, Result},
    fs::vfs::{Mode, OpenFlags},
    proc::thread::Thread,
    syscall::fs::utils::{AT_FDCWD, AT_REMOVEDIR, UserCStr, UserPtr, resolve_at_start},
};

/// System call `truncate(const char *path, off_t length)` (Syscall #76).
pub fn sys_truncate(path_ptr: usize, length: usize, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();
    let path = UserCStr::new(path_ptr).read_string(&vmar, 4096)?;

    crate::fs::vfs::truncate(&path, length)?;
    Ok(0)
}

/// System call `ftruncate(int fd, off_t length)` (Syscall #77).
pub fn sys_ftruncate(fd: i32, length: usize, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let file = proc.fd_table.lock().get(fd)?;

    if !file.flags.lock().contains(OpenFlags::WRITE) {
        return Err(Errno::EBADF);
    }
    if file.inode.is_dir() {
        return Err(Errno::EISDIR);
    }
    if !file.inode.is_file() {
        return Err(Errno::EINVAL);
    }

    let mut attr = file
        .inode
        .node_ops
        .getattr()
        .unwrap_or_else(|_| *file.inode.attr.read());
    attr.size = length;
    file.inode.node_ops.setattr(&attr)?;
    file.inode.set_size(length);
    file.inode.touch_mtime();

    Ok(0)
}

/// System call `rename(const char *oldpath, const char *newpath)` (Syscall #82).
pub fn sys_rename(oldpath_ptr: usize, newpath_ptr: usize, ctx: &mut UserContext) -> Result<usize> {
    sys_renameat(AT_FDCWD, oldpath_ptr, AT_FDCWD, newpath_ptr, ctx)
}

/// System call `mkdir(const char *pathname, mode_t mode)` (Syscall #83).
pub fn sys_mkdir(path_ptr: usize, mode: u32, ctx: &mut UserContext) -> Result<usize> {
    sys_mkdirat(AT_FDCWD, path_ptr, mode, ctx)
}

/// System call `rmdir(const char *pathname)` (Syscall #84).
pub fn sys_rmdir(path_ptr: usize, ctx: &mut UserContext) -> Result<usize> {
    sys_unlinkat(AT_FDCWD, path_ptr, AT_REMOVEDIR, ctx)
}

/// System call `link(const char *oldpath, const char *newpath)` (Syscall #86).
pub fn sys_link(oldpath_ptr: usize, newpath_ptr: usize, ctx: &mut UserContext) -> Result<usize> {
    sys_linkat(AT_FDCWD, oldpath_ptr, AT_FDCWD, newpath_ptr, 0, ctx)
}

/// System call `unlink(const char *pathname)` (Syscall #87).
pub fn sys_unlink(path_ptr: usize, ctx: &mut UserContext) -> Result<usize> {
    sys_unlinkat(AT_FDCWD, path_ptr, 0, ctx)
}

/// System call `symlink(const char *target, const char *linkpath)` (Syscall #88).
pub fn sys_symlink(target_ptr: usize, linkpath_ptr: usize, ctx: &mut UserContext) -> Result<usize> {
    sys_symlinkat(target_ptr, AT_FDCWD, linkpath_ptr, ctx)
}

/// System call `readlink(const char *pathname, char *buf, size_t bufsiz)` (Syscall #89).
pub fn sys_readlink(
    path_ptr: usize,
    buf_ptr: usize,
    bufsiz: usize,
    ctx: &mut UserContext,
) -> Result<usize> {
    sys_readlinkat(AT_FDCWD, path_ptr, buf_ptr, bufsiz, ctx)
}

/// System call `mkdirat(int dirfd, const char *pathname, mode_t mode)` (Syscall #258).
pub fn sys_mkdirat(
    dirfd: i32,
    path_ptr: usize,
    mode: u32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();
    let path = UserCStr::new(path_ptr).read_string(&vmar, 4096)?;

    let start = resolve_at_start(&proc, dirfd, &path)?;
    crate::fs::vfs::mkdir_at(&start, &path, Mode::from_bits_truncate(mode))?;
    Ok(0)
}

/// System call `mknodat(int dirfd, const char *pathname, mode_t mode, dev_t dev)` (Syscall #259).
pub fn sys_mknodat(
    dirfd: i32,
    path_ptr: usize,
    mode: u32,
    dev: u64,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();
    let path = UserCStr::new(path_ptr).read_string(&vmar, 4096)?;

    let start = resolve_at_start(&proc, dirfd, &path)?;
    crate::fs::vfs::mknod_at(&start, &path, Mode::from_bits_truncate(mode), dev)?;
    Ok(0)
}

/// System call `unlinkat(int dirfd, const char *pathname, int flags)` (Syscall #263).
pub fn sys_unlinkat(
    dirfd: i32,
    path_ptr: usize,
    flags: i32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();
    let path = UserCStr::new(path_ptr).read_string(&vmar, 4096)?;

    let start = resolve_at_start(&proc, dirfd, &path)?;
    if (flags & AT_REMOVEDIR) != 0 {
        crate::fs::vfs::rmdir_at(&start, &path)?;
    } else {
        crate::fs::vfs::unlink_at(&start, &path)?;
    }
    Ok(0)
}

/// System call `renameat(int olddirfd, const char *oldpath, int newdirfd, const char *newpath)` (Syscall #264).
pub fn sys_renameat(
    olddirfd: i32,
    oldpath_ptr: usize,
    newdirfd: i32,
    newpath_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();
    let oldpath = UserCStr::new(oldpath_ptr).read_string(&vmar, 4096)?;
    let newpath = UserCStr::new(newpath_ptr).read_string(&vmar, 4096)?;

    let old_start = resolve_at_start(&proc, olddirfd, &oldpath)?;
    let new_start = resolve_at_start(&proc, newdirfd, &newpath)?;

    crate::fs::vfs::rename_at(&old_start, &oldpath, &new_start, &newpath)?;
    Ok(0)
}

/// System call `renameat2(int olddirfd, const char *oldpath, int newdirfd, const char *newpath, unsigned int flags)` (Syscall #316).
pub fn sys_renameat2(
    olddirfd: i32,
    oldpath_ptr: usize,
    newdirfd: i32,
    newpath_ptr: usize,
    flags: u32,
    ctx: &mut UserContext,
) -> Result<usize> {
    if flags != 0 {
        return Err(Errno::EINVAL);
    }
    sys_renameat(olddirfd, oldpath_ptr, newdirfd, newpath_ptr, ctx)
}

/// System call `linkat(int olddirfd, const char *oldpath, int newdirfd, const char *newpath, int flags)` (Syscall #265).
pub fn sys_linkat(
    olddirfd: i32,
    oldpath_ptr: usize,
    newdirfd: i32,
    newpath_ptr: usize,
    flags: i32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();
    let oldpath = UserCStr::new(oldpath_ptr).read_string(&vmar, 4096)?;
    let newpath = UserCStr::new(newpath_ptr).read_string(&vmar, 4096)?;

    let old_start = resolve_at_start(&proc, olddirfd, &oldpath)?;
    let new_start = resolve_at_start(&proc, newdirfd, &newpath)?;

    crate::fs::vfs::link_at(&old_start, &oldpath, &new_start, &newpath, flags)?;
    Ok(0)
}

/// System call `symlinkat(const char *target, int newdirfd, const char *linkpath)` (Syscall #266).
pub fn sys_symlinkat(
    target_ptr: usize,
    newdirfd: i32,
    linkpath_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();
    let target = UserCStr::new(target_ptr).read_string(&vmar, 4096)?;
    let linkpath = UserCStr::new(linkpath_ptr).read_string(&vmar, 4096)?;

    let new_start = resolve_at_start(&proc, newdirfd, &linkpath)?;
    crate::fs::vfs::symlink_at(&target, &new_start, &linkpath)?;
    Ok(0)
}

/// System call `readlinkat(int dirfd, const char *pathname, char *buf, size_t bufsiz)` (Syscall #267).
pub fn sys_readlinkat(
    dirfd: i32,
    path_ptr: usize,
    buf_ptr: usize,
    bufsiz: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();
    let path = UserCStr::new(path_ptr).read_string(&vmar, 4096)?;

    let start = resolve_at_start(&proc, dirfd, &path)?;
    let target = crate::fs::vfs::readlink_at(&start, &path)?;

    let bytes = target.as_bytes();
    let copy_len = bytes.len().min(bufsiz);
    UserPtr::new(buf_ptr).write_bytes(&vmar, &bytes[..copy_len])?;

    Ok(copy_len)
}
