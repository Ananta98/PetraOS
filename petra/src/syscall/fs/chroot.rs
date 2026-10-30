// SPDX-License-Identifier: GPL-2.0

//! System calls `chroot(2)` and `pivot_root(2)` (Syscalls #161 and #155 on x86_64).

use ostd::arch::cpu::context::UserContext;

use crate::{
    fs::vfs::LookupFlags,
    proc::thread::Thread,
    syscall::fs::utils::{AT_FDCWD, UserCStr, resolve_at_start},
    uapi::errno::{Errno, Result},
};

/// System call `chroot(const char *path)` (Syscall #161).
pub fn sys_chroot(path_ptr: usize, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    if !proc.cred().is_superuser() {
        return Err(Errno::EPERM);
    }
    let vmar = proc.vmspace();
    let path = UserCStr::new(path_ptr).read_string(&vmar, 4096)?;
    if path.is_empty() {
        return Err(Errno::ENOENT);
    }

    let root_node = proc.root()?;
    let start = resolve_at_start(&proc, AT_FDCWD, &path)?;
    let target = crate::fs::lookup(root_node, start, &path, LookupFlags::empty())?;

    let inode = target.dentry.get_inode().ok_or(Errno::ENOENT)?;
    if !inode.is_dir() {
        return Err(Errno::ENOTDIR);
    }

    proc.set_root(target);
    Ok(0)
}

/// System call `pivot_root(const char *new_root, const char *put_old)` (Syscall #155).
pub fn sys_pivot_root(new_root_ptr: usize, put_old_ptr: usize, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    if !proc.cred().is_superuser() {
        return Err(Errno::EPERM);
    }
    let vmar = proc.vmspace();
    let new_path = UserCStr::new(new_root_ptr).read_string(&vmar, 4096)?;
    let old_path = UserCStr::new(put_old_ptr).read_string(&vmar, 4096)?;
    if new_path.is_empty() || old_path.is_empty() {
        return Err(Errno::ENOENT);
    }

    let current_root = proc.root()?;
    let new_start = resolve_at_start(&proc, AT_FDCWD, &new_path)?;
    let new_target = crate::fs::lookup(current_root.clone(), new_start, &new_path, LookupFlags::empty())?;

    let old_start = resolve_at_start(&proc, AT_FDCWD, &old_path)?;
    let old_target = crate::fs::lookup(current_root, old_start, &old_path, LookupFlags::empty())?;

    let new_inode = new_target.dentry.get_inode().ok_or(Errno::ENOENT)?;
    let old_inode = old_target.dentry.get_inode().ok_or(Errno::ENOENT)?;
    if !new_inode.is_dir() || !old_inode.is_dir() {
        return Err(Errno::ENOTDIR);
    }

    // Update global VFS root and calling process root and cwd
    crate::fs::vfs::set_root(new_target.clone());
    proc.set_root(new_target.clone());
    proc.set_cwd(new_target);

    Ok(0)
}
