/// Device and driver registry — global state and management functions.
///
/// Maintains two separate registries:
/// - **Driver registry**: keyed by driver name; used for lifecycle tracking.
/// - **Device registry**: keyed by device name; mirrors entries into
///   `/dev` via [`crate::fs::devfs`] for any device that exposes
///   [`InodeOps`].
use crate::device::device::{Device, DeviceType};
use crate::device::driver::Driver;
use crate::fs::vfs::FileType;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use ostd::sync::SpinLock;

// ---------------------------------------------------------------------------
// Global registries
// ---------------------------------------------------------------------------

static DRIVERS: SpinLock<BTreeMap<String, Arc<dyn Driver>>> =
    SpinLock::new(BTreeMap::new());

static DEVICES: SpinLock<BTreeMap<String, Arc<dyn Device>>> =
    SpinLock::new(BTreeMap::new());

// ---------------------------------------------------------------------------
// Registration helpers
// ---------------------------------------------------------------------------

/// Register a driver with the global driver registry.
///
/// Returns [`ostd::Error::InvalidArgs`] if a driver with the same name
/// is already registered.
pub fn register_driver(driver: Arc<dyn Driver>) -> Result<(), ostd::Error> {
    let mut drivers = DRIVERS.lock();
    let name = String::from(driver.name());
    if drivers.contains_key(&name) {
        return Err(ostd::Error::InvalidArgs);
    }
    drivers.insert(name, driver);
    Ok(())
}

/// Register a device with the global device registry.
///
/// If the device exposes [`InodeOps`][crate::fs::vfs::InodeOps], it is
/// also published as a node under `/dev` via
/// [`crate::fs::devfs::register_device`].
///
/// Returns [`ostd::Error::InvalidArgs`] if a device with the same name
/// is already registered.
pub fn register_device(device: Arc<dyn Device>) -> Result<(), ostd::Error> {
    let name = String::from(device.name());
    {
        let mut devices = DEVICES.lock();
        if devices.contains_key(&name) {
            return Err(ostd::Error::InvalidArgs);
        }
        devices.insert(name.clone(), device.clone());
    }

    if let Some(ops) = device.inode_ops() {
        let file_type = match device.device_type() {
            DeviceType::Char => FileType::CharDevice,
            DeviceType::Block => FileType::BlockDevice,
            // Net and Timer devices are not exposed as filesystem nodes.
            _ => return Ok(()),
        };
        crate::fs::devfs::register_device(&name, file_type, 0o660, ops)?;
    }

    Ok(())
}

/// Remove a device from the global device registry.
///
/// If the device had a `/dev` node, that node is also removed.
///
/// Returns [`ostd::Error::InvalidArgs`] if no device with `name` exists.
pub fn unregister_device(name: &str) -> Result<(), ostd::Error> {
    let dev = {
        let mut devices = DEVICES.lock();
        devices.remove(name).ok_or(ostd::Error::InvalidArgs)?
    };

    if dev.inode_ops().is_some() {
        let _ = crate::fs::devfs::unregister_device(name);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Subsystem initialisation
// ---------------------------------------------------------------------------

/// Initialise all device subsystems.
///
/// Called once from [`crate::kernel_main`] during boot, after the VM
/// subsystem is ready.
pub fn init() {
    crate::drivers::block::init();
    crate::drivers::char::init();
    crate::drivers::gpu::init();
    crate::drivers::timer::init();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(ktest)]
mod tests {
    use super::*;
    use crate::drivers::block::{BlockDevice, register_block_device};
    use crate::drivers::char::{CharDevice, register_char_device};
    use crate::fs::ramfs::RamFs;
    use crate::fs::vfs::{
        FileOps, SeekFrom, init_root_fs, mount, register_filesystem, resolve_path,
    };
    use ostd::prelude::ktest;

    struct MockChar {
        buf: SpinLock<alloc::vec::Vec<u8>>,
    }

    impl CharDevice for MockChar {
        fn read(&self, buf: &mut [u8]) -> Result<usize, ostd::Error> {
            let guard = self.buf.lock();
            let len = core::cmp::min(buf.len(), guard.len());
            buf[..len].copy_from_slice(&guard[..len]);
            Ok(len)
        }

        fn write(&self, buf: &[u8]) -> Result<usize, ostd::Error> {
            let mut guard = self.buf.lock();
            guard.clear();
            guard.extend_from_slice(buf);
            Ok(buf.len())
        }
    }

    struct MockBlock {
        data: SpinLock<alloc::vec::Vec<u8>>,
    }

    impl BlockDevice for MockBlock {
        fn block_size(&self) -> usize {
            128
        }

        fn num_blocks(&self) -> usize {
            4
        }

        fn read_blocks(&self, block_id: usize, buf: &mut [u8]) -> Result<(), ostd::Error> {
            if block_id >= 4 {
                return Err(ostd::Error::InvalidArgs);
            }
            let guard = self.data.lock();
            let offset = block_id * 128;
            buf[..128].copy_from_slice(&guard[offset..offset + 128]);
            Ok(())
        }

        fn write_blocks(&self, block_id: usize, buf: &[u8]) -> Result<(), ostd::Error> {
            if block_id >= 4 {
                return Err(ostd::Error::InvalidArgs);
            }
            let mut guard = self.data.lock();
            let offset = block_id * 128;
            guard[offset..offset + 128].copy_from_slice(&buf[..128]);
            Ok(())
        }
    }

    #[ktest]
    fn test_device_framework() {
        let ramfs = Arc::new(RamFs);
        let _ = register_filesystem(ramfs);
        let _ = init_root_fs("ramfs", &[]);

        let devfs = Arc::new(crate::fs::devfs::DevFs);
        let _ = register_filesystem(devfs);

        let root = crate::fs::vfs::ROOT_DENTRY
            .lock()
            .as_ref()
            .cloned()
            .unwrap();
        root.inode.mkdir("dev", 0o755).unwrap();

        mount("devfs", "/dev", 0, &[]).unwrap();

        let char_dev = Arc::new(MockChar {
            buf: SpinLock::new(alloc::vec::Vec::new()),
        });
        register_char_device("mock-char", char_dev.clone()).unwrap();

        let block_dev = Arc::new(MockBlock {
            data: SpinLock::new(alloc::vec![0u8; 512]),
        });
        register_block_device("mock-block", block_dev.clone()).unwrap();

        let char_dentry = resolve_path("/dev/mock-char").unwrap();
        assert_eq!(
            char_dentry.inode.metadata().unwrap().file_type,
            FileType::CharDevice
        );

        let mut char_ops = char_dentry.inode.open(0).unwrap();
        let mut write_offset = 0;
        char_ops
            .write(b"device framework tests", &mut write_offset)
            .unwrap();

        let mut read_buf = [0u8; 32];
        let mut read_offset = 0;
        let read_len = char_ops.read(&mut read_buf, &mut read_offset).unwrap();
        assert_eq!(read_len, 22);
        assert_eq!(&read_buf[..22], b"device framework tests");

        let block_dentry = resolve_path("/dev/mock-block").unwrap();
        assert_eq!(
            block_dentry.inode.metadata().unwrap().file_type,
            FileType::BlockDevice
        );

        let mut block_ops = block_dentry.inode.open(0).unwrap();
        let mut block_write_offset = 130;
        block_ops
            .write(b"block test data", &mut block_write_offset)
            .unwrap();

        let mut block_read_buf = [0u8; 15];
        let mut block_read_offset = 130;
        block_ops
            .read(&mut block_read_buf, &mut block_read_offset)
            .unwrap();
        assert_eq!(&block_read_buf, b"block test data");

        // Clean up registered devices from devfs to keep clean state.
        unregister_device("mock-char").unwrap();
        unregister_device("mock-block").unwrap();

        crate::fs::vfs::unregister_filesystem("devfs").unwrap();
        crate::fs::vfs::unregister_filesystem("ramfs").unwrap();
        *crate::fs::vfs::ROOT_DENTRY.lock() = None;
        *crate::fs::vfs::CWD_DENTRY.lock() = None;
        crate::fs::vfs::DENTRY_CACHE.clear();
    }
}
