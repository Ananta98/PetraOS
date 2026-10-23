// SPDX-License-Identifier: GPL-2.0

//! Unified Kernel Driver Model and Device Abstractions.
//!
//! Provides the payload traits devices expose to consumers
//! ([`CharacterDevice`], [`BlockDevice`], [`NetDevice`], [`AudioDevice`]) and
//! registers them through the generic device classes from [`device`]. Bus-level
//! discovery and driver binding live in [`device::bus`], used in particular by
//! the PCI subsystem.

pub mod audio;
pub mod block;
pub mod bus;
pub mod char;
pub mod device;
pub mod drm;
pub mod net;

use alloc::{string::String, sync::Arc, vec::Vec};

pub use audio::AudioDevice;
pub use device::{BusDevice, Driver};
pub use net::NetDevice;

use crate::uapi::errno::Result;

// ---------------------------------------------------------------------------
// Device payload traits
// ---------------------------------------------------------------------------

/// A character device that provides unstructured read/write character streams.
pub trait CharacterDevice: Send + Sync {
    /// The unique name of the character device (e.g. "mouse", "kbd").
    fn name(&self) -> String;

    /// Read data from the device into the provided buffer.
    fn read(&self, buf: &mut [u8]) -> Result<usize>;

    /// Write data to the device from the provided buffer.
    fn write(&self, buf: &[u8]) -> Result<usize>;

    /// Optional: Performs an I/O control operation.
    fn ioctl(&self, _cmd: u32, _arg: usize) -> Result<usize> {
        crate::return_errno!(ENOTTY, "inappropriate ioctl for device")
    }

    /// Optional: Reports readiness for poll/select/epoll.
    fn poll(&self) -> Result<u32> {
        Ok(0)
    }
}

/// Statistics for a block device, matching Linux `/proc/diskstats`.
#[derive(Debug, Clone, Copy, Default)]
pub struct BlockDeviceStats {
    /// Number of read operations completed successfully.
    pub reads_completed: u64,
    /// Number of read operations merged.
    pub reads_merged: u64,
    /// Number of sectors read (in 512-byte units).
    pub sectors_read: u64,
    /// Time spent reading (milliseconds).
    pub read_ms: u64,
    /// Number of write operations completed successfully.
    pub writes_completed: u64,
    /// Number of write operations merged.
    pub writes_merged: u64,
    /// Number of sectors written (in 512-byte units).
    pub sectors_written: u64,
    /// Time spent writing (milliseconds).
    pub write_ms: u64,
    /// Number of I/Os currently in progress.
    pub io_in_progress: u64,
    /// Time spent doing I/Os (milliseconds).
    pub io_ms: u64,
    /// Weighted time spent doing I/Os (milliseconds).
    pub weighted_io_ms: u64,
}

/// A block device that provides structured read/write operations for blocks.
pub trait BlockDevice: Send + Sync {
    /// The unique name of the block device (e.g. "nvme0n1").
    fn name(&self) -> String;

    /// Return the logical block size of the device in bytes (e.g. 512 or 4096).
    fn block_size(&self) -> usize;

    /// Return the total number of logical blocks on the device.
    fn block_count(&self) -> u64;

    /// Total capacity of the device in bytes.
    fn capacity(&self) -> u64 {
        self.block_count() * (self.block_size() as u64)
    }

    /// Read blocks from the device.
    fn read_blocks(&self, block_id: u64, buf: &mut [u8]) -> Result<usize>;

    /// Write blocks to the device.
    fn write_blocks(&self, block_id: u64, buf: &[u8]) -> Result<usize>;

    /// Optional: Flush written blocks.
    fn flush(&self) -> Result<()> {
        Ok(())
    }

    /// Return device major and minor numbers.
    fn device_number(&self) -> (u32, u32) {
        let name = self.name();
        if name.starts_with("sd") {
            let letter = name.as_bytes().get(2).copied().unwrap_or(b'a');
            let idx = (letter.saturating_sub(b'a')) as u32;
            (8, idx * 16)
        } else if name.starts_with("nvme") {
            (259, 0)
        } else if name.starts_with("vd") {
            (254, 0)
        } else {
            (252, 0)
        }
    }

    /// Return I/O statistics for this block device.
    fn stats(&self) -> BlockDeviceStats {
        BlockDeviceStats::default()
    }
}

// ---------------------------------------------------------------------------
// Device classes
// ---------------------------------------------------------------------------

/// The class of character stream devices.
pub struct CharClass;

impl device::Class for CharClass {
    type Device = dyn CharacterDevice;
}

/// The class of block storage devices.
pub struct BlockClass;

impl device::Class for BlockClass {
    type Device = dyn BlockDevice;
}

/// The class of network interfaces.
pub struct NetClass;

impl device::Class for NetClass {
    type Device = dyn NetDevice;
}

/// The class of audio PCM devices.
pub struct AudioClass;

impl device::Class for AudioClass {
    type Device = dyn AudioDevice;
}

static CHAR_CLASS: device::ClassHandle<CharClass> = device::ClassHandle::new();
static BLOCK_CLASS: device::ClassHandle<BlockClass> = device::ClassHandle::new();
static NET_CLASS: device::ClassHandle<NetClass> = device::ClassHandle::new();
static AUDIO_CLASS: device::ClassHandle<AudioClass> = device::ClassHandle::new();

// ---------------------------------------------------------------------------
// Registration helpers (also the module ABI entry points)
// ---------------------------------------------------------------------------

/// Registers a Character Device in the character device class.
pub fn register_chrdev(device: Arc<dyn CharacterDevice>) -> Result<()> {
    let name = device.name();
    CHAR_CLASS.register(&name, device)?;
    Ok(())
}

/// Unregisters a Character Device by name.
pub fn unregister_chrdev(name: &str) -> Result<()> {
    CHAR_CLASS.unregister(name)?;
    Ok(())
}

/// Registers a Block Device in the block device class.
pub fn register_blkdev(device: Arc<dyn BlockDevice>) -> Result<()> {
    let name = device.name();
    BLOCK_CLASS.register(&name, device)?;
    Ok(())
}

/// Unregisters a Block Device by name.
pub fn unregister_blkdev(name: &str) -> Result<()> {
    BLOCK_CLASS.unregister(name)?;
    Ok(())
}

/// Registers a Network Device in the network device class.
pub fn register_netdev(device: Arc<dyn NetDevice>) -> Result<()> {
    let name = device.name();
    NET_CLASS.register(&name, device)?;
    Ok(())
}

/// Unregisters a Network Device by name.
pub fn unregister_netdev(name: &str) -> Result<()> {
    NET_CLASS.unregister(name)?;
    Ok(())
}

/// Registers an Audio Device in the audio device class.
pub fn register_audiodev(device: Arc<dyn AudioDevice>) -> Result<()> {
    let name = device.name();
    AUDIO_CLASS.register(&name, device)?;
    Ok(())
}

/// Unregisters an Audio Device by name.
pub fn unregister_audiodev(name: &str) -> Result<()> {
    AUDIO_CLASS.unregister(name)?;
    Ok(())
}

/// Returns a snapshot of all registered Character Devices.
pub fn get_chrdevs() -> Vec<Arc<dyn CharacterDevice>> {
    CHAR_CLASS.payloads()
}

/// Returns a snapshot of all registered Block Devices.
pub fn get_blkdevs() -> Vec<Arc<dyn BlockDevice>> {
    BLOCK_CLASS.payloads()
}

/// Returns a snapshot of all registered Network Devices.
pub fn get_netdevs() -> Vec<Arc<dyn NetDevice>> {
    NET_CLASS.payloads()
}

/// Returns a snapshot of all registered Audio Devices.
pub fn get_audiodevs() -> Vec<Arc<dyn AudioDevice>> {
    AUDIO_CLASS.payloads()
}

/// Initializes the driver subsystem and probes system buses.
pub fn init() -> Result<()> {
    ostd::info!("Initializing driver subsystem...");
    bus::pci::init()
}
