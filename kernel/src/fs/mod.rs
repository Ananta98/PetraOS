pub mod devfs;
pub mod ext2;
pub mod fd;
pub mod ioctl;
pub mod ramfs;
pub mod vfs;

pub use vfs::dentry::Dentry;
pub use vfs::mount::{MOUNT_TABLE, Mount};
pub use vfs::path::{
    create_file, mkdir, readlink, rename, resolve_path, rmdir, stat, symlink, unlink,
};
pub use vfs::types::{
    File, FileOps, FileSystem, Inode, InodeOps, InodeType, O_CREAT, O_RDONLY, O_RDWR, O_WRONLY,
    SeekWhence, Stat, SuperBlock, VfsError, can_read, can_write,
};

/// Initialize Virtual File System (VFS) and mount root RamFS.
pub fn init() {
    log::info!("Initializing Virtual File System (VFS)...");

    let ramfs = ramfs::RamFs;
    MOUNT_TABLE
        .lock()
        .mount("/", &ramfs)
        .expect("Failed to mount RamFS root");

    let _ = mkdir("/dev");
    let _ = mkdir("/proc");
    let _ = mkdir("/mnt");

    // Check if a block storage device (AHCI / NVMe) exists and probe if Ext2 is supported
    let device_name = {
        let dm = crate::device::DEVICE_MANAGER.lock();
        let mut target_name = None;
        for dev in dm.get_devices() {
            let dev_lock = dev.lock();
            let name = dev_lock.as_ref().name();
            if name == "AHCI SATA Controller" || name == "NVMe Controller" {
                target_name = Some(name);
                break;
            }
        }
        target_name
    };

    if let Some(dev_name) = device_name {
        let ext2_fs = ext2::Ext2Fs::new(dev_name);
        match MOUNT_TABLE.lock().mount("/mnt", &ext2_fs) {
            Ok(_) => {
                log::info!(
                    "[VFS] Successfully mounted Ext2 filesystem on '{}' at /mnt",
                    dev_name
                );
            }
            Err(err) => {
                log::info!(
                    "[VFS] Ext2 mount skipped on '{}' at /mnt ({:?})",
                    dev_name,
                    err
                );
            }
        }
    }

    log::info!("VFS initialized successfully (Root RamFS mounted at /).");
}
