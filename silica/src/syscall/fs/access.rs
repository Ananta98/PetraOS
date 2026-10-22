// SPDX-License-Identifier: GPL-2.0

//! The `access`, `faccessat`, and `faccessat2` system calls (Syscalls #21, #269, and #439 on x86_64).

use ostd::arch::cpu::context::UserContext;

use crate::{
    api::errno::{Errno, Result},
    fs::vfs::{AccessFlags, LookupFlags},
    proc::thread::Thread,
    syscall::fs::utils::{
        AT_EMPTY_PATH, AT_FDCWD, AT_SYMLINK_NOFOLLOW, UserCStr, resolve_at_start,
    },
};

const F_OK: i32 = 0;
const X_OK: i32 = 1;
const W_OK: i32 = 2;
const R_OK: i32 = 4;

pub const AT_EACCESS: i32 = 0x200;

/// System call `faccessat2(int dirfd, const char *pathname, int mode, int flags)` (Syscall #439).
pub fn sys_faccessat2(
    dirfd: i32,
    path_ptr: usize,
    mode: i32,
    flags: i32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if (mode & !(R_OK | W_OK | X_OK | F_OK)) != 0 {
        return Err(Errno::EINVAL);
    }
    if (flags & !(AT_EACCESS | AT_SYMLINK_NOFOLLOW | AT_EMPTY_PATH)) != 0 {
        return Err(Errno::EINVAL);
    }

    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();

    let inode = if path_ptr == 0 {
        if (flags & AT_EMPTY_PATH) == 0 {
            return Err(Errno::EINVAL);
        }
        if dirfd == AT_FDCWD {
            proc.cwd()?.dentry.get_inode().ok_or(Errno::ENOENT)?
        } else {
            proc.file(dirfd)?.inode.clone()
        }
    } else {
        let path = UserCStr::new(path_ptr).read_string(&vmar, 4096)?;
        if path.is_empty() {
            if (flags & AT_EMPTY_PATH) != 0 {
                if dirfd == AT_FDCWD {
                    proc.cwd()?.dentry.get_inode().ok_or(Errno::ENOENT)?
                } else {
                    proc.file(dirfd)?.inode.clone()
                }
            } else {
                return Err(Errno::ENOENT);
            }
        } else {
            let start = resolve_at_start(&proc, dirfd, &path)?;
            let lookup_flags = if (flags & AT_SYMLINK_NOFOLLOW) != 0 {
                LookupFlags::NO_FOLLOW
            } else {
                LookupFlags::empty()
            };
            let root_node = crate::fs::root()?;
            let path_node = crate::fs::lookup(root_node, start, &path, lookup_flags)?;
            path_node.dentry.get_inode().ok_or(Errno::ENOENT)?
        }
    };

    if mode == F_OK {
        return Ok(0);
    }

    let mut access = AccessFlags::empty();
    if (mode & R_OK) != 0 {
        access |= AccessFlags::READ;
    }
    if (mode & W_OK) != 0 {
        access |= AccessFlags::WRITE;
    }
    if (mode & X_OK) != 0 {
        access |= AccessFlags::EXEC;
    }

    let attr = inode
        .node_ops
        .getattr()
        .unwrap_or_else(|_| *inode.attr.read());
    crate::fs::perms::check_permission(&attr, &proc.cred(), access)?;

    Ok(0)
}

/// System call `faccessat(int dirfd, const char *pathname, int mode)` (Syscall #269).
pub fn sys_faccessat(
    dirfd: i32,
    path_ptr: usize,
    mode: i32,
    ctx: &mut UserContext,
) -> Result<usize> {
    sys_faccessat2(dirfd, path_ptr, mode, 0, ctx)
}

/// System call `access(const char *pathname, int mode)` (Syscall #21).
pub fn sys_access(path_ptr: usize, mode: i32, ctx: &mut UserContext) -> Result<usize> {
    sys_faccessat2(AT_FDCWD, path_ptr, mode, 0, ctx)
}
