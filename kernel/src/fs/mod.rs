pub mod devfs;
pub mod exfat;
pub mod ext2;
pub mod fd_table;
pub mod procfs;
pub mod ramfs;
pub mod vfs;

use alloc::sync::Arc;
use vfs::Result;

/// Initialize the filesystem.
///
/// Registers all available filesystem types (ramfs, devfs, exfat, ext2, procfs),
/// initializes the root filesystem as a RAM filesystem (`ramfs`),
/// and mounts `devfs` at `/dev` and `procfs` at `/proc`.
pub fn init() -> Result<()> {
    vfs::register_filesystem(Arc::new(ramfs::RamFs))?;
    vfs::register_filesystem(Arc::new(devfs::DevFs))?;
    vfs::register_filesystem(Arc::new(exfat::ExFatFs))?;
    vfs::register_filesystem(Arc::new(ext2::Ext2Fs))?;
    vfs::register_filesystem(Arc::new(procfs::ProcFs))?;

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

    // Create /proc directory and mount procfs
    root.inode.mkdir("proc", 0o755)?;
    vfs::mount("procfs", "/proc", 0, &[])?;

    Ok(())
}
