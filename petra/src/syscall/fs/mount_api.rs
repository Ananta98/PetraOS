// SPDX-License-Identifier: GPL-2.0

//! The `fsopen(2)` family: `fsopen(430)`, `fsconfig(431)`, `fsmount(432)`,
//! `open_tree(428)`, `move_mount(429)` and `fspick(433)`.
//!
//! These are the new mount API Linux added for systemd/libmount; the work
//! itself funnels through the same core as legacy `mount(2)` so fstab units
//! see identical behavior.

use ostd::arch::cpu::context::UserContext;

use crate::{
    fs::{mount_api, mount_api::FsContext, vfs::lookup, vfs::mount::root, vfs::LookupFlags},
    proc::thread::Thread,
    syscall::fs::utils::{AT_FDCWD, UserCStr, resolve_at_start},
    uapi::errno::{Errno, Result},
};

/// Validates an fsopen family flag set.
const KNOWN_FSOPEN_FLAGS: u32 = mount_api::FSMOUNT_CLOEXEC;

/// `fsopen(const char *fsname, unsigned int flags)` (Syscall #430).
pub fn sys_fsopen(
    fsname_ptr: usize,
    flags: u32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if flags & !KNOWN_FSOPEN_FLAGS != 0 {
        return Err(Errno::EINVAL);
    }
    // fsopen surfaces filesystem types as "registered or not".
    if !root_uid_has_cap_sys_admin() {
        return Err(Errno::EPERM);
    }

    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();
    let fsname = UserCStr::new(fsname_ptr).read_string(&vmar, 64)?;
    if !registered_fstype(&fsname) {
        return Err(Errno::ENODEV);
    }

    let ctx = FsContext::new(&fsname);
    let cloexec = flags & mount_api::FSMOUNT_CLOEXEC != 0;
    let fd_flags = cloexec_flags(cloexec);
    let file = mount_api::create_fsctx_file(ctx, cloexec);
    let fd = proc.fd_table.lock().alloc_fd(file, fd_flags)?;
    Ok(fd as usize)
}

/// The fd-level `FD_CLOEXEC` counterpart of an API `*_CLOEXEC` bit.
fn cloexec_flags(cloexec: bool) -> crate::fs::fd::FdFlags {
    if cloexec {
        crate::fs::fd::FdFlags::FD_CLOEXEC
    } else {
        crate::fs::fd::FdFlags::empty()
    }
}

/// The filesystem types `mount(2)` understands, which is what "registered"
/// reduces to in PetraOS.
fn registered_fstype(fstype: &str) -> bool {
    matches!(
        fstype,
        "proc" | "procfs" | "sysfs" | "sys" | "tmpfs" | "ramfs" | "devfs" | "devtmpfs"
            | "devpts" | "cgroup" | "cgroup2" | "ext4" | "exfat"
    )
}

fn root_uid_has_cap_sys_admin() -> bool {
    Thread::proc_or_esrch()
        .map(|p| p.cred().cr_uid.is_root())
        .unwrap_or(false)
}

/// `fsconfig(int fs_fd, unsigned int cmd, const char *key, const void *value,
/// int aux)` (Syscall #431).
pub fn sys_fsconfig(
    fs_fd: i32,
    cmd: u32,
    key_ptr: usize,
    value_ptr: usize,
    aux: i32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();

    let file = proc.fd_table.lock().get(fs_fd)?;
    let ctx = file.as_fsctx().ok_or(Errno::EINVAL)?;

    // A flag / string command reads the key; FLAG/STRING variants use value.
    let key = UserCStr::new(key_ptr).read_string(&vmar, 255)?;
    let value = match cmd {
        cmd if cmd == mount_api::FSCONFIG_SET_STRING => {
            if aux != 0 {
                return Err(Errno::EINVAL);
            }
            UserCStr::new(value_ptr).read_string(&vmar, 4096)?
        }
        _ => alloc::string::String::new(),
    };

    ctx.fsconfig(cmd, &key, &value)?;
    Ok(0)
}

/// `fsmount(int fs_fd, unsigned int flags, unsigned int attr_flags)`
/// (Syscall #432).
pub fn sys_fsmount(
    fs_fd: i32,
    flags: u32,
    attr_flags: u32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if flags & !KNOWN_FSOPEN_FLAGS != 0 {
        return Err(Errno::EINVAL);
    }

    let proc = Thread::proc_or_esrch()?;
    let file = proc.fd_table.lock().get(fs_fd)?;
    let ctx = file.as_fsctx().ok_or(Errno::EINVAL)?;
    ctx.fsmount(attr_flags)?;
    Ok(0)
}

/// `open_tree(int dfd, const char *path, unsigned int flags)` (Syscall #428).
pub fn sys_open_tree(
    dirfd: i32,
    path_ptr: usize,
    flags: u32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if flags & !(mount_api::OPEN_TREE_CLONE | mount_api::OPEN_TREE_CLOEXEC) != 0 {
        return Err(Errno::EINVAL);
    }
    if flags & mount_api::OPEN_TREE_CLONE != 0 {
        // Mount cloning (per-mount views) is not supported; the caller gets a
        // plain descriptive failure rather than a misleading ENOSYS.
        crate::return_errno!(EINVAL, "open_tree clone not supported");
    }
    if !root_uid_has_cap_sys_admin() {
        return Err(Errno::EPERM);
    }

    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();
    let path = UserCStr::new(path_ptr).read_string(&vmar, 4096)?;
    let start = resolve_at_start(&proc, dirfd, &path)?;
    let root_node = root()?;
    lookup(root_node.clone(), start, &path, LookupFlags::DIRECTORY)?;

    let ctx = FsContext::new_bind(&path);
    let cloexec = flags & mount_api::OPEN_TREE_CLOEXEC != 0;
    let file = mount_api::create_fsctx_file(ctx, cloexec);
    let fd = proc.fd_table.lock().alloc_fd(file, cloexec_flags(cloexec))?;
    Ok(fd as usize)
}

/// `move_mount(int from_dfd, const char *from_path, int to_dfd,
/// const char *to_path, unsigned int flags)` (Syscall #429).
pub fn sys_move_mount(
    from_dirfd: i32,
    from_ptr: usize,
    to_dirfd: i32,
    to_ptr: usize,
    flags: u32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if flags != 0 {
        // F_/T_ pathname resolution flags have no meaning in our flat VFS.
        return Err(Errno::EINVAL);
    }
    if !root_uid_has_cap_sys_admin() {
        return Err(Errno::EPERM);
    }

    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();

    let file = proc.fd_table.lock().get(from_dirfd)?;
    let ctx = match file.as_fsctx() {
        Some(ctx) => ctx,
        None => {
            let _ = (from_ptr, to_dirfd);
            return Err(Errno::EINVAL);
        }
    };
    let _ = (from_ptr, AT_FDCWD);

    let (source, fstype, data, extra_flags, ro) = ctx.take_for_bind()?;
    let mut mount_flags = extra_flags;
    if ro {
        mount_flags |= crate::syscall::fs::MS_RDONLY;
    }

    let to_path = UserCStr::new(to_ptr).read_string(&vmar, 4096)?;
    let to_start = resolve_at_start(&proc, to_dirfd, &to_path)?;
    let root_node = root()?;
    lookup(root_node.clone(), to_start, &to_path, LookupFlags::DIRECTORY)?;

    crate::syscall::fs::perform_mount(&source, &to_path, &fstype, mount_flags, &data)?;
    Ok(0)
}

/// `fspick(int dfd, const char *path, unsigned int flags)` (Syscall #433).
pub fn sys_fspick(
    dirfd: i32,
    path_ptr: usize,
    flags: u32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if flags & !0b1111 != 0 {
        return Err(Errno::EINVAL);
    }
    if !root_uid_has_cap_sys_admin() {
        return Err(Errno::EPERM);
    }

    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();
    let path = UserCStr::new(path_ptr).read_string(&vmar, 4096)?;
    let start = resolve_at_start(&proc, dirfd, &path)?;
    let root_node = root()?;
    lookup(root_node.clone(), start, &path, LookupFlags::DIRECTORY)?;

    let ctx = FsContext::new_pick(&path);
    let cloexec = flags & mount_api::FSPICK_CLOEXEC != 0;
    let file = mount_api::create_fsctx_file(ctx, cloexec);
    let fd = proc.fd_table.lock().alloc_fd(file, cloexec_flags(cloexec))?;
    Ok(fd as usize)
}

/// `mount_setattr(int dfd, const char *path, unsigned int flags,
/// struct mount_attr *uattr, size_t usize)` (Syscall #442).
pub fn sys_mount_setattr(
    _dirfd: i32,
    _path_ptr: usize,
    _flags: u32,
    _uattr_ptr: usize,
    _usize_val: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    Ok(0)
}
