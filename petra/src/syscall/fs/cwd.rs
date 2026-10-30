// SPDX-License-Identifier: GPL-2.0

//! Working directory system calls: `getcwd`, `chdir`, and `fchdir`
//! (Syscalls #79, #80, and #81 on x86_64).

use alloc::vec::Vec;
use ostd::arch::cpu::context::UserContext;

use crate::{
    uapi::errno::{Errno, Result},
    fs::vfs::{AccessFlags, LookupFlags},
    proc::thread::Thread,
    syscall::fs::utils::{UserCStr, UserPtr},
};

/// System call `getcwd(char *buf, size_t size)` (Syscall #79).
pub fn sys_getcwd(buf_ptr: usize, size: usize, _ctx: &mut UserContext) -> Result<usize> {
    if size == 0 {
        return Err(Errno::EINVAL);
    }
    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();

    let cwd = proc.cwd()?;
    let path_str = cwd.abs_path();
    let bytes = path_str.as_bytes();
    let len = bytes.len() + 1; // Includes null terminator

    if size < len {
        return Err(Errno::ERANGE);
    }

    let mut user_buf = Vec::with_capacity(len);
    user_buf.extend_from_slice(bytes);
    user_buf.push(0);

    UserPtr::new(buf_ptr).write_bytes(&vmar, &user_buf)?;
    Ok(len)
}

/// System call `chdir(const char *path)` (Syscall #80).
pub fn sys_chdir(path_ptr: usize, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();
    let path = UserCStr::new(path_ptr).read_string(&vmar, 4096)?;

    let start = if path.starts_with('/') {
        proc.root()?
    } else {
        proc.cwd()?
    };
    let root = proc.root()?;
    let path_node = crate::fs::lookup(root, start, &path, LookupFlags::DIRECTORY)?;

    let inode = path_node.dentry.get_inode().ok_or(Errno::ENOENT)?;
    if !inode.is_dir() {
        return Err(Errno::ENOTDIR);
    }
    let attr = inode
        .node_ops
        .getattr()
        .unwrap_or_else(|_| *inode.attr.read());
    crate::fs::perms::check_permission(&attr, &proc.cred(), AccessFlags::EXEC)?;

    proc.set_cwd(path_node);
    Ok(0)
}

/// System call `fchdir(int fd)` (Syscall #81).
pub fn sys_fchdir(fd: i32, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let file = proc.file(fd)?;

    if !file.inode.is_dir() {
        return Err(Errno::ENOTDIR);
    }
    let attr = file
        .inode
        .node_ops
        .getattr()
        .unwrap_or_else(|_| *file.inode.attr.read());
    crate::fs::perms::check_permission(&attr, &proc.cred(), AccessFlags::EXEC)?;

    let path_node = file.path_node.clone().ok_or(Errno::EBADF)?;
    proc.set_cwd(path_node);
    Ok(0)
}
