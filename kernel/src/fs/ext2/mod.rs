pub mod bitmap;
pub mod dir;
pub mod file;
pub mod inode;
pub mod superblock;

use self::inode::{Ext2InodeOps, Ext2Volume};
use crate::fs::vfs::types::{FileSystem, Inode, InodeType, SuperBlock, VfsError};
use alloc::sync::Arc;

/// Ext2 Filesystem driver wrapper mapping to the VFS.
pub struct Ext2Fs {
    pub device_name: &'static str,
}

impl Ext2Fs {
    pub fn new(device_name: &'static str) -> Self {
        Self { device_name }
    }

    /// Auto-detect and mount Ext2 filesystem on available block storage devices.
    pub fn init() -> Result<(), &'static str> {
        let device_name = {
            let dm = crate::device::DEVICE_MANAGER.lock();
            let mut target_name = None;
            for dev in dm.get_devices() {
                let dev_lock = dev.lock();
                let name = dev_lock.as_ref().name();
                if name == "NVMe Controller" {
                    target_name = Some(name);
                    break;
                } else if name == "AHCI SATA Controller" && target_name.is_none() {
                    target_name = Some(name);
                }
            }
            target_name
        };

        if let Some(dev_name) = device_name {
            let ext2_fs = Ext2Fs::new(dev_name);
            let mut mt = crate::fs::vfs::mount::MOUNT_TABLE.lock();

            // First attempt to mount Ext2 at root '/' (if valid rootfs disk image present)
            match mt.mount("/", &ext2_fs) {
                Ok(_) => {
                    log::info!(
                        "[Ext2] Successfully mounted Ext2 filesystem on '{}' as root '/'",
                        dev_name
                    );
                }
                Err(_) => {
                    // Fallback to mounting at /mnt
                    match mt.mount("/mnt", &ext2_fs) {
                        Ok(_) => {
                            log::info!(
                                "[Ext2] Successfully mounted Ext2 filesystem on '{}' at /mnt",
                                dev_name
                            );
                        }
                        Err(err) => {
                            log::info!(
                                "[Ext2] Ext2 mount skipped on '{}' ({:?})",
                                dev_name,
                                err
                            );
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

impl FileSystem for Ext2Fs {
    fn name(&self) -> &'static str {
        "ext2"
    }

    /// Mount the Ext2 partition matching the configured block device name.
    fn mount(&self) -> Result<SuperBlock, VfsError> {
        let volume = Arc::new(Ext2Volume::new(self.device_name)?);

        // Root inode for Ext2 is always 2
        let root_inode = Arc::new(Inode {
            ino: 2,
            inode_type: InodeType::Directory,
            ops: Arc::new(Ext2InodeOps {
                volume: volume.clone(),
                ino: 2,
            }),
        });

        Ok(SuperBlock {
            fs_name: "ext2",
            root_inode,
            next_ino: core::sync::atomic::AtomicU64::new(volume.sb.inodes_count as u64 + 1),
            read_only: false,
        })
    }
}

crate::late_initcall!(Ext2Fs::init);
crate::MODULE_LICENSE!("BSD-2-Clause");
crate::MODULE_AUTHOR!("PetraOS Development Team");
crate::MODULE_DESCRIPTION!("Ext2 Filesystem Driver Subsystem");
