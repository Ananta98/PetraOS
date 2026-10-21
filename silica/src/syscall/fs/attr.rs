// SPDX-License-Identifier: GPL-2.0

//! System calls for file attributes and permissions:
//! `chmod`, `fchmod`, `fchmodat`, `chown`, `fchown`, `lchown`, `fchownat`, `umask`, `utimensat`, `futimesat`.

use alloc::sync::Arc;
use ostd::arch::cpu::context::UserContext;

use crate::{
    api::{
        cred::Uid,
        errno::{Errno, Result},
    },
    fs::vfs::{INode, LookupFlags, Mode, S_IFMT},
    proc::thread::Thread,
    syscall::fs::utils::{
        AT_EMPTY_PATH, AT_FDCWD, AT_SYMLINK_NOFOLLOW, UserCStr, UserPtr, resolve_at_start,
    },
};

const UTIME_NOW: i64 = 0x3fffffff;
const UTIME_OMIT: i64 = 0x3ffffffe;

/// System call `fchmodat2(int dirfd, const char *pathname, mode_t mode, int flags)` (Syscall #452).
pub fn sys_fchmodat2(
    dirfd: i32,
    path_ptr: usize,
    mode: u32,
    flags: i32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();

    let inode = if path_ptr == 0 {
        if (flags & AT_EMPTY_PATH) == 0 {
            return Err(Errno::EINVAL);
        }
        if dirfd == AT_FDCWD {
            proc.cwd()?.dentry.get_inode().ok_or(Errno::ENOENT)?
        } else {
            proc.fd_table.lock().get(dirfd)?.inode.clone()
        }
    } else {
        let path = UserCStr::new(path_ptr).read_string(&vmar, 4096)?;
        if path.is_empty() {
            if (flags & AT_EMPTY_PATH) != 0 {
                if dirfd == AT_FDCWD {
                    proc.cwd()?.dentry.get_inode().ok_or(Errno::ENOENT)?
                } else {
                    proc.fd_table.lock().get(dirfd)?.inode.clone()
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

    let cred = proc.cred();
    let mut attr = inode
        .node_ops
        .getattr()
        .unwrap_or_else(|_| *inode.attr.read());
    if !cred.is_superuser() && cred.cr_uid != Uid(attr.uid) {
        return Err(Errno::EPERM);
    }

    let file_type = attr.mode.bits() & S_IFMT;
    let new_mode = Mode::from_bits_truncate(file_type | (mode & 0o7777));
    attr.mode = new_mode;
    let _ = inode.node_ops.setattr(&attr);
    *inode.attr.write() = attr;
    inode.touch_ctime();

    Ok(0)
}

/// System call `fchmodat(int dirfd, const char *pathname, mode_t mode)` (Syscall #268).
pub fn sys_fchmodat(
    dirfd: i32,
    path_ptr: usize,
    mode: u32,
    ctx: &mut UserContext,
) -> Result<usize> {
    sys_fchmodat2(dirfd, path_ptr, mode, 0, ctx)
}

/// System call `chmod(const char *pathname, mode_t mode)` (Syscall #90).
pub fn sys_chmod(path_ptr: usize, mode: u32, ctx: &mut UserContext) -> Result<usize> {
    sys_fchmodat2(AT_FDCWD, path_ptr, mode, 0, ctx)
}

/// System call `fchmod(int fd, mode_t mode)` (Syscall #91).
pub fn sys_fchmod(fd: i32, mode: u32, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let file = proc.fd_table.lock().get(fd)?;

    let cred = proc.cred();
    let mut attr = file
        .inode
        .node_ops
        .getattr()
        .unwrap_or_else(|_| *file.inode.attr.read());
    if !cred.is_superuser() && cred.cr_uid != Uid(attr.uid) {
        return Err(Errno::EPERM);
    }

    let file_type = attr.mode.bits() & S_IFMT;
    attr.mode = Mode::from_bits_truncate(file_type | (mode & 0o7777));
    let _ = file.inode.node_ops.setattr(&attr);
    *file.inode.attr.write() = attr;
    file.inode.touch_ctime();

    Ok(0)
}

/// System call `fchownat(int dirfd, const char *pathname, uid_t owner, gid_t group, int flags)` (Syscall #260).
pub fn sys_fchownat(
    dirfd: i32,
    path_ptr: usize,
    owner: u32,
    group: u32,
    flags: i32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();

    let inode = if path_ptr == 0 {
        if (flags & AT_EMPTY_PATH) == 0 {
            return Err(Errno::EINVAL);
        }
        if dirfd == AT_FDCWD {
            proc.cwd()?.dentry.get_inode().ok_or(Errno::ENOENT)?
        } else {
            proc.fd_table.lock().get(dirfd)?.inode.clone()
        }
    } else {
        let path = UserCStr::new(path_ptr).read_string(&vmar, 4096)?;
        if path.is_empty() {
            if (flags & AT_EMPTY_PATH) != 0 {
                if dirfd == AT_FDCWD {
                    proc.cwd()?.dentry.get_inode().ok_or(Errno::ENOENT)?
                } else {
                    proc.fd_table.lock().get(dirfd)?.inode.clone()
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

    let cred = proc.cred();
    if !cred.is_superuser() {
        return Err(Errno::EPERM);
    }

    let mut attr = inode
        .node_ops
        .getattr()
        .unwrap_or_else(|_| *inode.attr.read());
    if owner != 0xffffffff {
        attr.uid = owner;
    }
    if group != 0xffffffff {
        attr.gid = group;
    }
    let _ = inode.node_ops.setattr(&attr);
    *inode.attr.write() = attr;
    inode.touch_ctime();

    Ok(0)
}

/// System call `chown(const char *pathname, uid_t owner, gid_t group)` (Syscall #92).
pub fn sys_chown(path_ptr: usize, owner: u32, group: u32, ctx: &mut UserContext) -> Result<usize> {
    sys_fchownat(AT_FDCWD, path_ptr, owner, group, 0, ctx)
}

/// System call `lchown(const char *pathname, uid_t owner, gid_t group)` (Syscall #94).
pub fn sys_lchown(path_ptr: usize, owner: u32, group: u32, ctx: &mut UserContext) -> Result<usize> {
    sys_fchownat(AT_FDCWD, path_ptr, owner, group, AT_SYMLINK_NOFOLLOW, ctx)
}

/// System call `fchown(int fd, uid_t owner, gid_t group)` (Syscall #93).
pub fn sys_fchown(fd: i32, owner: u32, group: u32, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let file = proc.fd_table.lock().get(fd)?;

    let cred = proc.cred();
    if !cred.is_superuser() {
        return Err(Errno::EPERM);
    }

    let mut attr = file
        .inode
        .node_ops
        .getattr()
        .unwrap_or_else(|_| *file.inode.attr.read());
    if owner != 0xffffffff {
        attr.uid = owner;
    }
    if group != 0xffffffff {
        attr.gid = group;
    }
    let _ = file.inode.node_ops.setattr(&attr);
    *file.inode.attr.write() = attr;
    file.inode.touch_ctime();

    Ok(0)
}

/// System call `umask(mode_t mask)` (Syscall #95).
pub fn sys_umask(mask: u32, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let mut table = proc.fd_table.lock();
    let old = table.cmask;
    table.cmask = mask & 0o777;
    Ok(old as usize)
}

/// System call `utimensat(int dirfd, const char *pathname, const struct timespec times[2], int flags)` (Syscall #280).
pub fn sys_utimensat(
    dirfd: i32,
    path_ptr: usize,
    times_ptr: usize,
    flags: i32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();

    let inode: Arc<INode> = if path_ptr == 0 {
        let file = proc.fd_table.lock().get(dirfd)?;
        file.inode.clone()
    } else {
        let path = UserCStr::new(path_ptr).read_string(&vmar, 4096)?;
        if path.is_empty() {
            if (flags & AT_EMPTY_PATH) != 0 {
                if dirfd == AT_FDCWD {
                    proc.cwd()?.dentry.get_inode().ok_or(Errno::ENOENT)?
                } else {
                    proc.fd_table.lock().get(dirfd)?.inode.clone()
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

    let cred = proc.cred();
    let attr = *inode.attr.read();
    if !cred.is_superuser() && cred.cr_uid != Uid(attr.uid) {
        return Err(Errno::EPERM);
    }

    let now = crate::sched::now() / crate::sched::TIMER_HZ;
    let mut cur_times = inode.times();

    if times_ptr == 0 {
        cur_times.atime = now;
        cur_times.mtime = now;
    } else {
        let mut raw = [0u8; 32];
        UserPtr::new(times_ptr).read_bytes(&vmar, &mut raw)?;

        let atime_sec = i64::from_ne_bytes(raw[0..8].try_into().unwrap());
        let atime_nsec = i64::from_ne_bytes(raw[8..16].try_into().unwrap());
        let mtime_sec = i64::from_ne_bytes(raw[16..24].try_into().unwrap());
        let mtime_nsec = i64::from_ne_bytes(raw[24..32].try_into().unwrap());

        if atime_nsec == UTIME_NOW {
            cur_times.atime = now;
        } else if atime_nsec != UTIME_OMIT {
            if atime_sec < 0 || !(0..1_000_000_000).contains(&atime_nsec) {
                return Err(Errno::EINVAL);
            }
            cur_times.atime = atime_sec as u64;
        }

        if mtime_nsec == UTIME_NOW {
            cur_times.mtime = now;
        } else if mtime_nsec != UTIME_OMIT {
            if mtime_sec < 0 || !(0..1_000_000_000).contains(&mtime_nsec) {
                return Err(Errno::EINVAL);
            }
            cur_times.mtime = mtime_sec as u64;
        }
    }

    inode.set_times(cur_times);
    inode.touch_ctime();

    Ok(0)
}

/// System call `futimesat(int dirfd, const char *pathname, const struct timeval times[2])` (Syscall #261).
pub fn sys_futimesat(
    dirfd: i32,
    path_ptr: usize,
    times_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();

    let inode: Arc<INode> = if path_ptr == 0 {
        let file = proc.fd_table.lock().get(dirfd)?;
        file.inode.clone()
    } else {
        let path = UserCStr::new(path_ptr).read_string(&vmar, 4096)?;
        if path.is_empty() {
            if dirfd == AT_FDCWD {
                proc.cwd()?.dentry.get_inode().ok_or(Errno::ENOENT)?
            } else {
                proc.fd_table.lock().get(dirfd)?.inode.clone()
            }
        } else {
            let start = resolve_at_start(&proc, dirfd, &path)?;
            let root_node = crate::fs::root()?;
            let path_node = crate::fs::lookup(root_node, start, &path, LookupFlags::empty())?;
            path_node.dentry.get_inode().ok_or(Errno::ENOENT)?
        }
    };

    let cred = proc.cred();
    let attr = *inode.attr.read();
    if !cred.is_superuser() && cred.cr_uid != Uid(attr.uid) {
        return Err(Errno::EPERM);
    }

    let now = crate::sched::now() / crate::sched::TIMER_HZ;
    let mut cur_times = inode.times();

    if times_ptr == 0 {
        cur_times.atime = now;
        cur_times.mtime = now;
    } else {
        let mut raw = [0u8; 32];
        UserPtr::new(times_ptr).read_bytes(&vmar, &mut raw)?;

        let atime_sec = i64::from_ne_bytes(raw[0..8].try_into().unwrap());
        let atime_usec = i64::from_ne_bytes(raw[8..16].try_into().unwrap());
        let mtime_sec = i64::from_ne_bytes(raw[16..24].try_into().unwrap());
        let mtime_usec = i64::from_ne_bytes(raw[24..32].try_into().unwrap());

        if atime_sec < 0 || atime_usec < 0 || mtime_sec < 0 || mtime_usec < 0 {
            return Err(Errno::EINVAL);
        }

        cur_times.atime = atime_sec as u64;
        cur_times.mtime = mtime_sec as u64;
    }

    inode.set_times(cur_times);
    inode.touch_ctime();

    Ok(0)
}
