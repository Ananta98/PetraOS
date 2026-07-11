pub mod devfs;
pub mod exfat;
pub mod ext2;
pub mod fd_table;
pub mod ramfs;
pub mod vfs;

use alloc::sync::Arc;
use vfs::Result;

/// Initialize the filesystem.
///
/// Registers all available filesystem types (ramfs, devfs, exfat, ext2),
/// initializes the root filesystem as a RAM filesystem (`ramfs`),
/// and mounts `devfs` at `/dev` to expose devices.
pub fn init() -> Result<()> {
    vfs::register_filesystem(Arc::new(ramfs::RamFs))?;
    vfs::register_filesystem(Arc::new(devfs::DevFs))?;
    vfs::register_filesystem(Arc::new(exfat::ExFatFs))?;
    vfs::register_filesystem(Arc::new(ext2::Ext2Fs))?;

    // Setup root filesystem
    vfs::init_root_fs("ramfs", &[])?;

    // Create /dev directory and mount devfs
    let root = vfs::ROOT_DENTRY
        .lock()
        .as_ref()
        .cloned()
        .ok_or(ostd::Error::InvalidArgs)?;
    root.inode.mkdir("dev", 0o755)?;
    vfs::mount("devfs", "/dev", 0, &[])?;

    Ok(())
}
