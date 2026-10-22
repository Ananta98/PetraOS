// SPDX-License-Identifier: GPL-2.0

//! The `mount` and `umount2` system calls (Syscalls #165 and #166 on x86_64).

use alloc::sync::Arc;
use ostd::arch::cpu::context::UserContext;

use crate::{
    uapi::errno::Result,
    fs::{
        devfs::DevFs,
        procfs::ProcFs,
        sysfs::SysFs,
        tmpfs::TmpFs,
        vfs::{
            FileSystem, LookupFlags,
            dcache::DEntry,
            lookup,
            mount::{Mount, register_mount, root, unregister_mount},
        },
    },
    proc::thread::Thread,
    syscall::fs::utils::UserCStr,
};

pub const MS_RDONLY: usize = 1;
pub const MS_NOSUID: usize = 2;
pub const MS_NODEV: usize = 4;
pub const MS_NOEXEC: usize = 8;
pub const MS_REMOUNT: usize = 32;
pub const MS_BIND: usize = 4096;

pub const MNT_FORCE: i32 = 1;
pub const MNT_DETACH: i32 = 2;
pub const MNT_EXPIRE: i32 = 4;
pub const UMOUNT_NOFOLLOW: i32 = 8;

/// System call `mount(const char *source, const char *target, const char *filesystemtype, unsigned long mountflags, const void *data)` (Syscall #165).
pub fn sys_mount(
    dev_name_ptr: usize,
    dir_name_ptr: usize,
    type_ptr: usize,
    flags: usize,
    data_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();

    let target = UserCStr::new(dir_name_ptr).read_string(&vmar, 4096)?;
    let source = if dev_name_ptr != 0 {
        UserCStr::new(dev_name_ptr)
            .read_string(&vmar, 4096)
            .unwrap_or_default()
    } else {
        alloc::string::String::new()
    };
    let fstype = if type_ptr != 0 {
        UserCStr::new(type_ptr)
            .read_string(&vmar, 64)
            .unwrap_or_default()
    } else {
        alloc::string::String::new()
    };
    let _data = if data_ptr != 0 {
        UserCStr::new(data_ptr)
            .read_string(&vmar, 1024)
            .unwrap_or_default()
    } else {
        alloc::string::String::new()
    };

    let root_node = root()?;
    let target_node = lookup(
        root_node.clone(),
        root_node.clone(),
        &target,
        LookupFlags::DIRECTORY,
    )?;

    // Handle MS_REMOUNT
    if (flags & MS_REMOUNT) != 0 {
        return Ok(0);
    }

    // Handle MS_BIND
    if (flags & MS_BIND) != 0 {
        let source_node = lookup(
            root_node.clone(),
            root_node.clone(),
            &source,
            LookupFlags::DIRECTORY,
        )?;
        let mount_dentry = Arc::new(DEntry::new(
            alloc::string::String::from(target.trim_end_matches('/')),
            source_node.dentry.get_inode(),
            Some(Arc::downgrade(&target_node.dentry)),
        ));
        let new_mount = Arc::new(Mount::new(mount_dentry, source_node.mount.fs.clone()));
        target_node.mount(new_mount)?;
        register_mount(&source, &target, source_node.mount.fs.name(), "rw,bind");
        return Ok(0);
    }

    // Normal filesystem mounting
    let fs_instance: Arc<dyn FileSystem> = match fstype.as_str() {
        "proc" | "procfs" => Arc::new(ProcFs::new()),
        "sysfs" | "sys" => Arc::new(SysFs::new()),
        "tmpfs" | "ramfs" => Arc::new(TmpFs::new()),
        "devfs" => Arc::new(DevFs::new()),
        "devpts" => Arc::new(TmpFs::new()),
        "ext4" | "exfat" => {
            let dev_name = source.trim_start_matches("/dev/").trim_start_matches('/');
            return crate::fs::registry::mount_device(&target, dev_name, &fstype).map(|_| 0);
        }
        _ => {
            crate::return_errno!(ENODEV, "unsupported filesystem type");
        }
    };

    let root_inode = fs_instance.root()?;
    let mount_dentry = Arc::new(DEntry::new(
        alloc::string::String::from(target.trim_end_matches('/')),
        Some(root_inode),
        Some(Arc::downgrade(&target_node.dentry)),
    ));
    let new_mount = Arc::new(Mount::new(Arc::clone(&mount_dentry), fs_instance.clone()));
    target_node.mount(new_mount)?;

    let opts = if (flags & MS_RDONLY) != 0 {
        "ro"
    } else {
        "rw,relatime"
    };
    let src = if source.is_empty() { &fstype } else { &source };
    register_mount(src, &target, fs_instance.name(), opts);

    ostd::info!("VFS: Mounted \"{}\" ({}) at \"{}\"", src, fstype, target);
    Ok(0)
}

/// System call `umount2(const char *target, int flags)` (Syscall #166).
pub fn sys_umount2(target_ptr: usize, _flags: i32, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();

    let target = UserCStr::new(target_ptr).read_string(&vmar, 4096)?;
    let root_node = root()?;
    let target_node = lookup(
        root_node.clone(),
        root_node,
        &target,
        LookupFlags::DIRECTORY,
    )?;

    target_node.umount()?;
    unregister_mount(&target);

    ostd::info!("VFS: Unmounted \"{}\"", target);
    Ok(0)
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;

    #[ktest]
    fn mount_registry_and_tracking() {
        register_mount("test_dev", "/mnt/test", "tmpfs", "rw");
        let mounts = crate::fs::vfs::mount::get_mounts();
        assert!(
            mounts
                .iter()
                .any(|m| m.target == "/mnt/test" && m.fstype == "tmpfs")
        );

        unregister_mount("/mnt/test");
        let mounts_after = crate::fs::vfs::mount::get_mounts();
        assert!(!mounts_after.iter().any(|m| m.target == "/mnt/test"));
    }
}
