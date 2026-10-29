// SPDX-License-Identifier: GPL-2.0

//! Linux `struct file_operations` and `struct cdev` abstractions.
//!
//! Bridges Linux character device file operations into PetraOS character devices
//! and `/dev` nodes.

use alloc::{string::String, sync::Arc};
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::drivers::{CharacterDevice, register_chrdev, unregister_chrdev};
use crate::uapi::errno::Result;

/// Linux `struct file_operations`: operations implemented by character drivers.
pub trait FileOperations: Send + Sync + 'static {
    /// Read data from the device (`read`).
    fn read(&self, _buf: &mut [u8], _offset: usize) -> Result<usize> {
        crate::return_errno!(EINVAL, "read not supported on this device")
    }

    /// Write data to the device (`write`).
    fn write(&self, _buf: &[u8], _offset: usize) -> Result<usize> {
        crate::return_errno!(EINVAL, "write not supported on this device")
    }

    /// Perform device-specific I/O control operation (`unlocked_ioctl`).
    fn unlocked_ioctl(&self, _cmd: u32, _arg: usize) -> Result<usize> {
        crate::return_errno!(ENOTTY, "inappropriate ioctl for device")
    }

    /// Poll readiness for reading/writing (`poll`).
    fn poll(&self) -> Result<u32> {
        Ok(0)
    }

    /// Called when the device node is opened (`open`).
    fn open(&self) -> Result<()> {
        Ok(())
    }

    /// Called when the last file descriptor is closed (`release`).
    fn release(&self) -> Result<()> {
        Ok(())
    }
}

/// Linux `struct cdev`: character device registration handle.
pub struct CDev {
    name: String,
    major: u32,
    minor: u32,
    fops: Arc<dyn FileOperations>,
}

impl CDev {
    /// Allocates and initializes a new character device.
    pub fn new(name: impl Into<String>, major: u32, minor: u32, fops: Arc<dyn FileOperations>) -> Self {
        Self {
            name: name.into(),
            major,
            minor,
            fops,
        }
    }

    /// Name of the character device (appears under `/dev/<name>`).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Device major number.
    pub fn major(&self) -> u32 {
        self.major
    }

    /// Device minor number.
    pub fn minor(&self) -> u32 {
        self.minor
    }

    /// Associated file operations.
    pub fn fops(&self) -> &Arc<dyn FileOperations> {
        &self.fops
    }
}

/// Adapter converting a `CDev` with `FileOperations` into PetraOS `CharacterDevice`.
struct CDevAdapter {
    cdev: CDev,
}

impl CharacterDevice for CDevAdapter {
    fn name(&self) -> String {
        self.cdev.name.clone()
    }

    fn read(&self, buf: &mut [u8]) -> Result<usize> {
        self.cdev.fops.read(buf, 0)
    }

    fn write(&self, buf: &[u8]) -> Result<usize> {
        self.cdev.fops.write(buf, 0)
    }

    fn ioctl(&self, cmd: u32, arg: usize) -> Result<usize> {
        self.cdev.fops.unlocked_ioctl(cmd, arg)
    }

    fn poll(&self) -> Result<u32> {
        self.cdev.fops.poll()
    }
}

/// Global dynamic major number generator.
static DYNAMIC_MAJOR: AtomicUsize = AtomicUsize::new(240);

/// Allocates a dynamic character device major number region.
pub fn alloc_chrdev_region() -> u32 {
    DYNAMIC_MAJOR.fetch_add(1, Ordering::Relaxed) as u32
}

/// Linux `cdev_add`: registers a character device into the system.
pub fn cdev_add(cdev: CDev) -> Result<()> {
    let adapter = Arc::new(CDevAdapter { cdev });
    register_chrdev(adapter)
}

/// Linux `cdev_del`: unregisters a character device by name.
pub fn cdev_del(name: &str) -> Result<()> {
    unregister_chrdev(name)
}
