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
pub mod core;
pub use self::core as device;
pub mod drm;
pub mod input;
pub mod net;

use alloc::{string::String, sync::Arc, vec::Vec};
use spin::Once;

pub use audio::AudioDevice;
pub use self::core::{BusDevice, Driver};
pub use net::NetDevice;

use self::core::{Attr, ClassDevice, ClassHandle, DevNum, DeviceId};
use crate::uapi::errno::{Errno, Result};

// ---------------------------------------------------------------------------
// Device payload traits
// ---------------------------------------------------------------------------

/// A character device that provides unstructured read/write character streams.
pub trait CharacterDevice: Send + Sync {
    /// The unique name of the character device (e.g. "mouse", "kbd").
    fn name(&self) -> String;

    /// Read data from the device into the provided buffer.
    fn read(&self, buf: &mut [u8]) -> Result<usize>;

    /// Optional: Read data from the device at a specific offset.
    fn read_at(&self, _offset: u64, buf: &mut [u8]) -> Result<usize> {
        self.read(buf)
    }

    /// Write data to the device from the provided buffer.
    fn write(&self, buf: &[u8]) -> Result<usize>;

    /// Optional: Write data to the device at a specific offset.
    fn write_at(&self, _offset: u64, buf: &[u8]) -> Result<usize> {
        self.write(buf)
    }

    /// Optional: Called when a device node is opened.
    fn open(&self) -> Result<()> {
        Ok(())
    }

    /// Optional: Called when a device node is released/closed.
    fn release(&self) -> Result<()> {
        Ok(())
    }

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
    const NAME: &'static str = "char";
    type Device = Arc<dyn CharacterDevice>;
}

/// The class of block storage devices.
pub struct BlockClass;

impl device::Class for BlockClass {
    const NAME: &'static str = "block";
    type Device = Arc<dyn BlockDevice>;
}

/// The class of network interfaces.
pub struct NetClass;

const NET_CLASS_ATTRS: &[Attr<ClassDevice<NetClass>>] = &[
    Attr::ro("address", |dev, w| {
        let mac = dev.payload().mac_address();
        writeln!(
            w,
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
        )?;
        Ok(())
    }),
    Attr::ro("mtu", |dev, w| {
        writeln!(w, "{}", dev.payload().mtu())?;
        Ok(())
    }),
];

impl device::Class for NetClass {
    const NAME: &'static str = "net";
    type Device = Arc<dyn NetDevice>;

    fn dev_attrs(&self) -> &'static [Attr<ClassDevice<Self>>] {
        NET_CLASS_ATTRS
    }
}

/// The class of audio PCM devices.
pub struct AudioClass;

impl device::Class for AudioClass {
    const NAME: &'static str = "sound";
    type Device = Arc<dyn AudioDevice>;
}

static CHAR_CLASS: Once<Arc<ClassHandle<CharClass>>> = Once::new();
static BLOCK_CLASS: Once<Arc<ClassHandle<BlockClass>>> = Once::new();
static NET_CLASS: Once<Arc<ClassHandle<NetClass>>> = Once::new();
static AUDIO_CLASS: Once<Arc<ClassHandle<AudioClass>>> = Once::new();

fn char_class() -> &'static Arc<ClassHandle<CharClass>> {
    CHAR_CLASS.call_once(|| {
        core::init();
        device::class::register(CharClass).expect("failed to register char class")
    })
}

fn block_class() -> &'static Arc<ClassHandle<BlockClass>> {
    BLOCK_CLASS.call_once(|| {
        core::init();
        device::class::register(BlockClass).expect("failed to register block class")
    })
}

fn net_class() -> &'static Arc<ClassHandle<NetClass>> {
    NET_CLASS.call_once(|| {
        core::init();
        device::class::register(NetClass).expect("failed to register net class")
    })
}

fn audio_class() -> &'static Arc<ClassHandle<AudioClass>> {
    AUDIO_CLASS.call_once(|| {
        core::init();
        device::class::register(AudioClass).expect("failed to register sound class")
    })
}

// ---------------------------------------------------------------------------
// Registration helpers (also the module ABI entry points)
// ---------------------------------------------------------------------------

/// Registers a Character Device in the character device class.
pub fn register_chrdev(device: Arc<dyn CharacterDevice>) -> Result<()> {
    let name = device.name();
    let dev = ClassDevice::builder(char_class(), name, device).build();
    device::add_device(&dev).map_err(Errno::from)
}

/// Unregisters a Character Device by name.
pub fn unregister_chrdev(name: &str) -> Result<()> {
    let dev = char_class().find_device(name).ok_or(Errno::ENODEV)?;
    device::remove_device(&dev).map_err(Errno::from)
}

/// Registers a Block Device in the block device class.
pub fn register_blkdev(device: Arc<dyn BlockDevice>) -> Result<()> {
    let name = device.name();
    let (maj, min) = device.device_number();
    let dev = ClassDevice::builder(block_class(), name, device)
        .devnum(DevNum::block(DeviceId::new(maj, min)))
        .build();
    device::add_device(&dev).map_err(Errno::from)
}

/// Unregisters a Block Device by name.
pub fn unregister_blkdev(name: &str) -> Result<()> {
    let dev = block_class().find_device(name).ok_or(Errno::ENODEV)?;
    device::remove_device(&dev).map_err(Errno::from)
}

/// Registers a Network Device in the network device class.
pub fn register_netdev(device: Arc<dyn NetDevice>) -> Result<()> {
    let name = device.name();
    let dev = ClassDevice::builder(net_class(), name, device).build();
    device::add_device(&dev).map_err(Errno::from)
}

/// Unregisters a Network Device by name.
pub fn unregister_netdev(name: &str) -> Result<()> {
    let dev = net_class().find_device(name).ok_or(Errno::ENODEV)?;
    device::remove_device(&dev).map_err(Errno::from)
}

/// Registers an Audio Device in the audio device class.
pub fn register_audiodev(device: Arc<dyn AudioDevice>) -> Result<()> {
    let name = device.name();
    let dev = ClassDevice::builder(audio_class(), name, device).build();
    device::add_device(&dev).map_err(Errno::from)
}

/// Unregisters an Audio Device by name.
pub fn unregister_audiodev(name: &str) -> Result<()> {
    let dev = audio_class().find_device(name).ok_or(Errno::ENODEV)?;
    device::remove_device(&dev).map_err(Errno::from)
}

/// Returns a snapshot of all registered Character Devices.
pub fn get_chrdevs() -> Vec<Arc<dyn CharacterDevice>> {
    char_class()
        .devices()
        .into_iter()
        .map(|d| Arc::clone(d.payload()))
        .collect()
}

/// Finds a Character Device by name.
pub fn get_chrdev(name: &str) -> Option<Arc<dyn CharacterDevice>> {
    char_class()
        .find_device(name)
        .map(|d| Arc::clone(d.payload()))
}

/// Returns a snapshot of all registered Block Devices.
pub fn get_blkdevs() -> Vec<Arc<dyn BlockDevice>> {
    block_class()
        .devices()
        .into_iter()
        .map(|d| Arc::clone(d.payload()))
        .collect()
}

/// Finds a Block Device by name.
pub fn get_blkdev(name: &str) -> Option<Arc<dyn BlockDevice>> {
    block_class()
        .find_device(name)
        .map(|d| Arc::clone(d.payload()))
}

/// Returns a snapshot of all registered Network Devices.
pub fn get_netdevs() -> Vec<Arc<dyn NetDevice>> {
    net_class()
        .devices()
        .into_iter()
        .map(|d| Arc::clone(d.payload()))
        .collect()
}

/// Finds a Network Device by name.
pub fn get_netdev(name: &str) -> Option<Arc<dyn NetDevice>> {
    net_class()
        .find_device(name)
        .map(|d| Arc::clone(d.payload()))
}

/// Returns a snapshot of all registered Audio Devices.
pub fn get_audiodevs() -> Vec<Arc<dyn AudioDevice>> {
    audio_class()
        .devices()
        .into_iter()
        .map(|d| Arc::clone(d.payload()))
        .collect()
}

/// Finds an Audio Device by name.
pub fn get_audiodev(name: &str) -> Option<Arc<dyn AudioDevice>> {
    audio_class()
        .find_device(name)
        .map(|d| Arc::clone(d.payload()))
}

/// Initializes the driver subsystem and probes system buses.
pub fn init() -> Result<()> {
    ostd::info!("Initializing driver subsystem...");
    core::init();
    let _ = (char_class(), block_class(), net_class(), audio_class());
    bus::pci::init()
}
