// SPDX-License-Identifier: GPL-2.0

//! The `listxattr` system call (Syscall #194 on x86_64).

use ostd::arch::cpu::context::UserContext;

use crate::{
    api::errno::{Errno, Result},
    fs::vfs::LookupFlags,
    proc::thread::Thread,
    syscall::fs::utils::{AT_FDCWD, UserCStr, resolve_at_start},
};

/// System call `listxattr(const char *pathname, char *list, size_t size)` (Syscall #194).
pub fn sys_listxattr(
    path_ptr: usize,
    _list_ptr: usize,
    _size: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();

    let path = UserCStr::new(path_ptr).read_string(&vmar, 4096)?;
    let root_node = crate::fs::root()?;
    let start = if path.is_empty() {
        return Err(Errno::ENOENT);
    } else {
        resolve_at_start(&proc, AT_FDCWD, &path)?
    };
    let path_node = crate::fs::lookup(root_node, start, &path, LookupFlags::empty())?;
    let _ = path_node.dentry.get_inode().ok_or(Errno::ENOENT)?;
    // No extended attributes are stored; report an empty list.
    Ok(0)
}
