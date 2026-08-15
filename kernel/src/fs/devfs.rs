use alloc::sync::Arc;
use core::sync::atomic::AtomicU64;

use crate::device::DEVICE_MANAGER;
use crate::fs::ramfs::RamDirInode;
use crate::fs::vfs::dentry::Dentry;
use crate::fs::vfs::mount::MOUNT_TABLE;
use crate::fs::vfs::types::{
    FileOps, FileSystem, Inode, InodeOps, InodeType, SuperBlock, VfsError,
};

/// Device filesystem, mounted at `/dev`.
pub struct DevFs;

impl FileSystem for DevFs {
    fn name(&self) -> &'static str {
        "devfs"
    }

    fn mount(&self) -> Result<SuperBlock, VfsError> {
        let next_ino = AtomicU64::new(1);
        let ino = next_ino.fetch_add(1, core::sync::atomic::Ordering::Relaxed);

        let root_inode = Arc::new(Inode {
            ino,
            inode_type: InodeType::Directory,
            ops: Arc::new(RamDirInode::new()),
        });

        Ok(SuperBlock {
            fs_name: "devfs",
            root_inode,
            next_ino,
            read_only: false,
        })
    }
}

/// Inode for the `/dev/console` device.
pub struct ConsoleInode;

impl InodeOps for ConsoleInode {
    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        Ok(Arc::new(ConsoleFileOps))
    }
}

/// File operations for the console character device.
pub struct ConsoleFileOps;

impl FileOps for ConsoleFileOps {
    fn read(&self, _offset: usize, _buf: &mut [u8]) -> Result<usize, VfsError> {
        Ok(0)
    }

    fn write(&self, _offset: usize, buf: &[u8]) -> Result<usize, VfsError> {
        if let Ok(s) = core::str::from_utf8(buf) {
            log::info!("[CONSOLE] {}", s.trim_end());
        }
        Ok(buf.len())
    }
}

/// Inode for block devices registered in devfs.
pub struct BlockDeviceInode {
    pub device_name: &'static str,
}

impl InodeOps for BlockDeviceInode {
    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        Ok(Arc::new(BlockDeviceFileOps {
            device_name: self.device_name,
        }))
    }
}

pub struct BlockDeviceFileOps {
    pub device_name: &'static str,
}

impl FileOps for BlockDeviceFileOps {
    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, VfsError> {
        let dm = DEVICE_MANAGER.read();
        for dev_arc in dm.get_devices() {
            let mut dev_lock = dev_arc.lock();
            if dev_lock.name() == self.device_name {
                if let Some(block_dev) = dev_lock.as_block_device_mut() {
                    let block_size = block_dev.block_size();
                    let block_id = (offset / block_size) as u64;
                    let mut read_bytes = 0;
                    let mut temp_buf = alloc::vec![0u8; block_size];

                    while read_bytes < buf.len() {
                        let current_block = block_id + (read_bytes / block_size) as u64;
                        block_dev
                            .read_block(current_block, &mut temp_buf)
                            .map_err(|_| VfsError::NotSupported)?;

                        let remaining = buf.len() - read_bytes;
                        let chunk = core::cmp::min(remaining, block_size);
                        buf[read_bytes..read_bytes + chunk].copy_from_slice(&temp_buf[..chunk]);
                        read_bytes += chunk;
                    }
                    return Ok(read_bytes);
                }
            }
        }
        Err(VfsError::NotFound)
    }

    fn write(&self, offset: usize, buf: &[u8]) -> Result<usize, VfsError> {
        let dm = DEVICE_MANAGER.read();
        for dev_arc in dm.get_devices() {
            let mut dev_lock = dev_arc.lock();
            if dev_lock.name() == self.device_name {
                if let Some(block_dev) = dev_lock.as_block_device_mut() {
                    let block_size = block_dev.block_size();
                    let block_id = (offset / block_size) as u64;
                    let mut written_bytes = 0;

                    while written_bytes < buf.len() {
                        let current_block = block_id + (written_bytes / block_size) as u64;
                        let remaining = buf.len() - written_bytes;
                        let chunk = core::cmp::min(remaining, block_size);

                        if chunk == block_size {
                            block_dev
                                .write_block(
                                    current_block,
                                    &buf[written_bytes..written_bytes + chunk],
                                )
                                .map_err(|_| VfsError::NotSupported)?;
                        } else {
                            let mut temp_buf = alloc::vec![0u8; block_size];
                            block_dev
                                .read_block(current_block, &mut temp_buf)
                                .map_err(|_| VfsError::NotSupported)?;
                            temp_buf[..chunk]
                                .copy_from_slice(&buf[written_bytes..written_bytes + chunk]);
                            block_dev
                                .write_block(current_block, &temp_buf)
                                .map_err(|_| VfsError::NotSupported)?;
                        }
                        written_bytes += chunk;
                    }
                    return Ok(written_bytes);
                }
            }
        }
        Err(VfsError::NotFound)
    }
}

impl DevFs {
    /// Mount the device filesystem at `/dev` and register core device nodes.
    pub fn init() -> Result<(), &'static str> {
        let mut mt = MOUNT_TABLE.write();
        let dev_mount = mt
            .mount("/dev", &DevFs)
            .map_err(|_| "Failed to mount devfs at /dev")?;

        // Register console character device
        let console_ino = dev_mount.superblock.alloc_ino();
        let console_inode = Arc::new(Inode {
            ino: console_ino,
            inode_type: InodeType::CharDevice,
            ops: Arc::new(ConsoleInode),
        });
        Dentry::add_child(&dev_mount.root_dentry, "console".into(), console_inode);

        // Scan DEVICE_MANAGER and dynamically register discovered block devices
        let dm = DEVICE_MANAGER.read();
        for dev_arc in dm.get_devices() {
            let dev_lock = dev_arc.lock();
            if dev_lock.dev_type() == crate::device::DeviceType::Block {
                let dev_name = dev_lock.name();
                let vfs_name = if dev_name.contains("AHCI") {
                    "sda"
                } else if dev_name.contains("NVMe") {
                    "nvme0n1"
                } else {
                    continue;
                };

                let block_ino = dev_mount.superblock.alloc_ino();
                let block_inode = Arc::new(Inode {
                    ino: block_ino,
                    inode_type: InodeType::BlockDevice,
                    ops: Arc::new(BlockDeviceInode {
                        device_name: dev_name,
                    }),
                });
                Dentry::add_child(&dev_mount.root_dentry, vfs_name.into(), block_inode);
            }
        }

        log::info!("[DevFS] Mounted /dev successfully.");
        Ok(())
    }
}

/// Legacy wrapper for mounting devfs.
pub fn mount_devfs() {
    let _ = DevFs::init();
}

crate::fs_initcall!(DevFs::init);
crate::MODULE_LICENSE!("BSD-2-Clause");
crate::MODULE_AUTHOR!("PetraOS Development Team");
crate::MODULE_DESCRIPTION!("Device Filesystem Subsystem");

