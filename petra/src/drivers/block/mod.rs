// SPDX-License-Identifier: GPL-2.0

//! Block Device Driver Abstraction.
//!
//! Provides the core [`BlockDevice`] trait required by filesystem implementations
//! such as `ext2`, `fat32`, or `exfat`, plus the registration helper that
//! adds block devices to the `block` class.

pub mod ahci;
pub mod counters;
pub mod nvme;
pub mod virtio;

use alloc::sync::Arc;

use crate::uapi::errno::Result;

pub use counters::BlockDeviceCounters;
pub use crate::drivers::BlockDevice;

/// Registers a block device into the block device class.
pub fn register_block_device(name: &str, device: Arc<dyn BlockDevice>) -> Result<()> {
    ostd::info!(
        "Block: Registered block device \"/dev/{}\" (size: {} MB, block_size: {} B)",
        name,
        device.capacity() / (1024 * 1024),
        device.block_size()
    );
    crate::drivers::register_blkdev(device)?;
    Ok(())
}

/// Retrieves a block device by name.
pub fn get_block_device(name: &str) -> Option<Arc<dyn BlockDevice>> {
    crate::drivers::get_blkdev(name)
}
