use crate::fs::vfs::mount;
use crate::syscall::SyscallResult;
use crate::vm::vma::VmaManager;
use crate::syscall::to_continue_unit;
use ostd::Error;

/// System call entry: mount a filesystem.
///
/// Resolves the filesystem driver by name, resolves the mount target path,
/// and mounts the filesystem at the target.
pub(crate) fn syscall_mount(
    arg0: usize,
    arg1: usize,
    arg2: usize,
    arg3: usize,
    arg4: usize,
    _: usize,
    vm: &VmaManager,
) -> SyscallResult {
    let flags = arg2 as u32;

    let fs_type_res = super::read_user_string(vm, arg0);
    let target_path_res = super::read_user_string(vm, arg1);
    let data_res = super::read_user_slice(vm, arg3, arg4);

    match (fs_type_res, target_path_res, data_res) {
        (Ok(fs_type), Ok(target_path), Ok(data)) => {
            to_continue_unit(mount(&fs_type, &target_path, flags, &data))
        }
        _ => to_continue_unit(Err(Error::AccessDenied)),
    }
}

// =============================================================================
// Unit Tests Block
// =============================================================================

#[cfg(ktest)]
mod tests {
    use super::*;
    use crate::fs::ramfs::RamFs;
    use crate::fs::vfs::{FileType, init_root_fs, register_filesystem, resolve_path};
    use alloc::sync::Arc;
    use ostd::prelude::ktest;

    #[ktest]
    fn test_sys_mount() {
        // Register and initialize root filesystem
        let ramfs = Arc::new(RamFs);
        let _ = register_filesystem(ramfs.clone());
        let _ = init_root_fs("ramfs", &[]);

        let root = crate::fs::vfs::ROOT_DENTRY
            .lock()
            .as_ref()
            .cloned()
            .unwrap();

        // Create mountpoint directory
        root.inode.mkdir("mnt", 0o755).unwrap();

        // Call mount to mount another instance of ramfs at /mnt
        mount("ramfs", "/mnt", 0, &[]).unwrap();

        // Verify the mountpoint is successfully resolved
        let mnt_dentry = resolve_path("/mnt").unwrap();
        assert_eq!(
            mnt_dentry.inode.metadata().unwrap().file_type,
            FileType::Directory
        );

        // Verify we can create a file /mnt/test.txt inside the mounted filesystem
        let file_inode = mnt_dentry.inode.create("test.txt", 0o644).unwrap();
        assert_eq!(file_inode.metadata().unwrap().file_type, FileType::Regular);

        // Cleanup
        crate::fs::vfs::unregister_filesystem("ramfs").unwrap();
        *crate::fs::vfs::ROOT_DENTRY.lock() = None;
        *crate::fs::vfs::CWD_DENTRY.lock() = None;
        crate::fs::vfs::DENTRY_CACHE.clear();
    }
}
