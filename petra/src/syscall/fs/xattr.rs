// SPDX-License-Identifier: GPL-2.0

//! Extended attribute (xattr) system calls (Syscalls #188-#199 on x86_64).

use alloc::sync::Arc;
use ostd::arch::cpu::context::UserContext;

use crate::{
    fs::vfs::{LookupFlags, PathNode},
    proc::{proc::Proc, thread::Thread},
    syscall::fs::utils::{AT_FDCWD, UserCStr, resolve_at_start},
    uapi::errno::{Errno, Result},
};

const XATTR_CREATE: u32 = 0x1;
const XATTR_REPLACE: u32 = 0x2;
const XATTR_SIZE_MAX: usize = 65536;

fn lookup_path(proc: &Arc<Proc>, path_ptr: usize, flags: LookupFlags) -> Result<PathNode> {
    let vmar = proc.vmspace();
    let path = UserCStr::new(path_ptr).read_string(&vmar, 4096)?;
    if path.is_empty() {
        return Err(Errno::ENOENT);
    }
    let root_node = crate::fs::root()?;
    let start = resolve_at_start(proc, AT_FDCWD, &path)?;
    let path_node = crate::fs::lookup(root_node, start, &path, flags)?;
    let _ = path_node.dentry.get_inode().ok_or(Errno::ENOENT)?;
    Ok(path_node)
}

fn validate_name(proc: &Arc<Proc>, name_ptr: usize) -> Result<alloc::string::String> {
    if name_ptr == 0 {
        return Err(Errno::EFAULT);
    }
    let vmar = proc.vmspace();
    let name = UserCStr::new(name_ptr).read_string(&vmar, 256)?;
    if name.is_empty() {
        return Err(Errno::EINVAL);
    }
    Ok(name)
}

fn validate_set_args(
    val_ptr: usize,
    size: usize,
    flags: u32,
) -> Result<()> {
    if flags > (XATTR_CREATE | XATTR_REPLACE) {
        return Err(Errno::EINVAL);
    }
    if size > XATTR_SIZE_MAX {
        return Err(Errno::E2BIG);
    }
    if size > 0 && val_ptr == 0 {
        return Err(Errno::EFAULT);
    }
    // If explicit replacement requested, since we don't persist xattrs yet, report ENODATA
    if flags == XATTR_REPLACE {
        return Err(Errno::ENODATA);
    }
    Ok(())
}

/// System call `setxattr(const char *pathname, const char *name, const void *value, size_t size, int flags)` (Syscall #188).
pub fn sys_setxattr(
    path_ptr: usize,
    name_ptr: usize,
    val_ptr: usize,
    size: usize,
    flags: u32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let _ = lookup_path(&proc, path_ptr, LookupFlags::empty())?;
    let _name = validate_name(&proc, name_ptr)?;
    validate_set_args(val_ptr, size, flags)?;
    Ok(0)
}

/// System call `lsetxattr(const char *pathname, const char *name, const void *value, size_t size, int flags)` (Syscall #189).
pub fn sys_lsetxattr(
    path_ptr: usize,
    name_ptr: usize,
    val_ptr: usize,
    size: usize,
    flags: u32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let _ = lookup_path(&proc, path_ptr, LookupFlags::NO_FOLLOW)?;
    let _name = validate_name(&proc, name_ptr)?;
    validate_set_args(val_ptr, size, flags)?;
    Ok(0)
}

/// System call `fsetxattr(int fd, const char *name, const void *value, size_t size, int flags)` (Syscall #190).
pub fn sys_fsetxattr(
    fd: i32,
    name_ptr: usize,
    val_ptr: usize,
    size: usize,
    flags: u32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let _file = proc.file(fd)?;
    let _name = validate_name(&proc, name_ptr)?;
    validate_set_args(val_ptr, size, flags)?;
    Ok(0)
}

/// System call `getxattr(const char *pathname, const char *name, void *value, size_t size)` (Syscall #191).
pub fn sys_getxattr(
    path_ptr: usize,
    name_ptr: usize,
    _val_ptr: usize,
    _size: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let _ = lookup_path(&proc, path_ptr, LookupFlags::empty())?;
    let _name = validate_name(&proc, name_ptr)?;
    Err(Errno::ENODATA)
}

/// System call `lgetxattr(const char *pathname, const char *name, void *value, size_t size)` (Syscall #192).
pub fn sys_lgetxattr(
    path_ptr: usize,
    name_ptr: usize,
    _val_ptr: usize,
    _size: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let _ = lookup_path(&proc, path_ptr, LookupFlags::NO_FOLLOW)?;
    let _name = validate_name(&proc, name_ptr)?;
    Err(Errno::ENODATA)
}

/// System call `fgetxattr(int fd, const char *name, void *value, size_t size)` (Syscall #193).
pub fn sys_fgetxattr(
    fd: i32,
    name_ptr: usize,
    _val_ptr: usize,
    _size: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let _file = proc.file(fd)?;
    let _name = validate_name(&proc, name_ptr)?;
    Err(Errno::ENODATA)
}

/// System call `listxattr(const char *pathname, char *list, size_t size)` (Syscall #194).
pub fn sys_listxattr(
    path_ptr: usize,
    _list_ptr: usize,
    _size: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let _ = lookup_path(&proc, path_ptr, LookupFlags::empty())?;
    Ok(0)
}

/// System call `llistxattr(const char *pathname, char *list, size_t size)` (Syscall #195).
pub fn sys_llistxattr(
    path_ptr: usize,
    _list_ptr: usize,
    _size: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let _ = lookup_path(&proc, path_ptr, LookupFlags::NO_FOLLOW)?;
    Ok(0)
}

/// System call `flistxattr(int fd, char *list, size_t size)` (Syscall #196).
pub fn sys_flistxattr(
    fd: i32,
    _list_ptr: usize,
    _size: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let _file = proc.file(fd)?;
    Ok(0)
}

/// System call `removexattr(const char *pathname, const char *name)` (Syscall #197).
pub fn sys_removexattr(
    path_ptr: usize,
    name_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let _ = lookup_path(&proc, path_ptr, LookupFlags::empty())?;
    let _name = validate_name(&proc, name_ptr)?;
    Err(Errno::ENODATA)
}

/// System call `lremovexattr(const char *pathname, const char *name)` (Syscall #198).
pub fn sys_lremovexattr(
    path_ptr: usize,
    name_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let _ = lookup_path(&proc, path_ptr, LookupFlags::NO_FOLLOW)?;
    let _name = validate_name(&proc, name_ptr)?;
    Err(Errno::ENODATA)
}

/// System call `fremovexattr(int fd, const char *name)` (Syscall #199).
pub fn sys_fremovexattr(
    fd: i32,
    name_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let _file = proc.file(fd)?;
    let _name = validate_name(&proc, name_ptr)?;
    Err(Errno::ENODATA)
}
