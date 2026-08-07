use crate::fs::devfs::mount_devfs;
use crate::fs::ext2::mount_ext2;
use crate::fs::fd::setup_init_std_fds;
use crate::fs::procfs::mount_procfs;
use crate::fs::ramfs::RamFs;
use crate::fs::tmpfs::mount_tmpfs;
use crate::fs::vfs::dentry::Dentry;
use crate::fs::vfs::mount::MOUNT_TABLE;
use alloc::sync::Arc;

/// Initialize the Virtual File System (VFS).
pub fn init() {
    let root_dentry = mount_root();
    create_mount_points(&root_dentry);
    mount_devfs();
    mount_tmpfs();
    mount_procfs();
    // mount_ext2();
    setup_init_std_fds();
    log::info!("VFS: system filesystems mounted and initialized successfully.");
}

/// Mount the primary RAM filesystem at `/`.
fn mount_root() -> Arc<Dentry> {
    let mut mt = MOUNT_TABLE.lock();
    let root_mount = mt.mount("/", &RamFs).expect("Failed to mount ramfs at /");
    root_mount.root_dentry.clone()
}

/// Create necessary mountpoint directories on the root filesystem.
fn create_mount_points(root_dentry: &Arc<Dentry>) {
    let dirs = ["dev", "tmp", "proc", "mnt"];
    for dir in &dirs {
        root_dentry
            .inode
            .ops
            .mkdir(dir)
            .expect("Failed to create mountpoint");
        Dentry::add_child(
            root_dentry,
            (*dir).into(),
            root_dentry
                .inode
                .ops
                .lookup(dir)
                .expect("Failed to lookup mountpoint"),
        );
    }
}
