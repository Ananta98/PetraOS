// SPDX-License-Identifier: GPL-2.0

//! Virtio transport bring-up and the [`VirtioDevice`] integration point.
//!
//! The transport owns one modern virtio PCI device from caplist walk to
//! connected queues: capability discovery lives in [`super::caps`], queue
//! rings in [`super::queue`], and the activation protocol (reset →
//! acknowledge → features → queues → driver-ok) here. Device drivers
//! register a [`VirtioDevice`] with the bus, which claims matching
//! transports.

use alloc::sync::Arc;
use alloc::vec::Vec;
use ostd::io::IoMem;
use ostd::mm::io::{VmIo, VmIoOnce};
use spin::Mutex;

use super::caps::{find_caps, map_cap};
use crate::api::errno::{Errno, Result};
use crate::drivers::bus::pci::{PCI_DEVICES, PciDevice};

// `virtio_pci_common_cfg` register offsets (virtio 1.0 spec).
const CFG_MAGIC_VALUE: usize = 0x00;
const CFG_VERSION: usize = 0x04;
const CFG_DEVICE_FEATURE: usize = 0x10;
const CFG_DEVICE_FEATURE_SEL: usize = 0x14;
const CFG_DRIVER_FEATURE: usize = 0x18;
const CFG_DRIVER_FEATURE_SEL: usize = 0x1C;
const CFG_MSIX_CONFIG: usize = 0x20;
const CFG_NUM_QUEUES: usize = 0x22;
const CFG_STATUS: usize = 0x24;
const CFG_QUEUE_SELECT: usize = 0x28;
const CFG_QUEUE_SIZE: usize = 0x2A;
const CFG_QUEUE_MSIX_VECTOR: usize = 0x2C;
const CFG_QUEUE_ENABLE: usize = 0x2E;
const CFG_QUEUE_NOTIFY_OFF: usize = 0x30;
const CFG_QUEUE_DESC: usize = 0x38;
const CFG_QUEUE_DESC_HI: usize = 0x3C;
const CFG_QUEUE_AVAIL: usize = 0x40;
const CFG_QUEUE_AVAIL_HI: usize = 0x44;
const CFG_QUEUE_USED: usize = 0x48;
const CFG_QUEUE_USED_HI: usize = 0x4C;

/// The `virt` magic value of the common config.
const VIRTIO_MAGIC: u32 = 0x7472_6976;
/// The common config layout spoken here is `virtio 1.0`.
const VIRTIO_VERSION: u32 = 1;
/// `VIRTIO_F_VERSION_1`: mandatory feature bit 32 of modern devices.
pub const VIRTIO_F_VERSION_1: u64 = 1 << 32;

// `device_status` bit values.
pub const STATUS_RESET: u8 = 0;
pub const STATUS_ACKNOWLEDGE: u8 = 1;
pub const STATUS_DRIVER: u8 = 2;
pub const STATUS_DRIVER_OK: u8 = 4;
pub const STATUS_FEATURES_OK: u8 = 8;
pub const STATUS_FAILED: u8 = 128;

/// A device driver exposed by one of the virtio device drivers.
pub trait VirtioDevice: Send + Sync {
    /// The PCI device id (e.g. `0x1050` for virtio-gpu).
    fn device_id(&self) -> u16;

    /// Human-readable device name.
    fn name(&self) -> &'static str;

    /// Number of queues to activate (control-path-only devices: 1).
    fn queue_count(&self) -> usize {
        1
    }

    /// The device features to accept; the framework additionally always
    /// keeps `VIRTIO_F_VERSION_1`.
    fn accept_features(&self, device_features: u64) -> u64 {
        device_features & VIRTIO_F_VERSION_1
    }

    /// Called with the fully wired transport.
    fn on_init(&self, transport: &Arc<VirtioTransport>) -> Result<()>;
}

/// One attached virtio PCI device, from caplist walk to live queues.
pub struct VirtioTransport {
    device: PciDevice,
    /// The `virtio_pci_common_cfg` root window.
    common: IoMem,
    /// The shared notification window; each queue rings at its own offset
    /// times [`notify_multiplier`].
    notify: IoMem,
    notify_multiplier: u32,
    /// The device-specific config window, if present.
    device_cfg: Option<IoMem>,
    /// The activated queues, in creation order.
    queues: Mutex<Vec<Arc<super::queue::VirtQueue>>>,
    /// The single attached device driver.
    driver: Mutex<Option<Arc<dyn VirtioDevice>>>,
}

impl VirtioTransport {
    /// Walks the caps of `device` and maps the virtio PCI windows.
    pub fn new(device: PciDevice) -> Result<Arc<Self>> {
        let caps =
            find_caps(&device)?
                ;
        let common = map_cap(&device, caps.common.as_ref().unwrap())?;
        let notify_cap = caps.notify.unwrap();
        let notify = map_cap(&device, &notify_cap)?;
        let device_cfg = match caps.device {
            Some(device_cap) => Some(map_cap(&device, &device_cap)?),
            None => None,
        };

        Ok(Arc::new(Self {
            device,
            common,
            notify,
            notify_multiplier: notify_cap.notify_multiplier,
            device_cfg,
            queues: Mutex::new(Vec::new()),
            driver: Mutex::new(None),
        }))
    }

    /// The PCI device id.
    pub fn pci_device_id(&self) -> u16 {
        self.device.device_id
    }

    /// Reads a `__le32` from the device-specific config.
    pub fn device_read32(&self, offset: usize) -> Result<u32> {
        self.device_cfg
            .as_ref()
            .ok_or(Errno::ENODEV)?
            .read_val::<u32>(offset)
            .map_err(|_| Errno::EIO)
    }

    /// Reads a `__le16` from the device-specific config.
    pub fn device_read16(&self, offset: usize) -> Result<u16> {
        self.device_cfg
            .as_ref()
            .ok_or(Errno::ENODEV)?
            .read_val::<u16>(offset)
            .map_err(|_| Errno::EIO)
    }

    /// The configured queues, in activation order.
    pub fn queues(&self) -> Vec<Arc<super::queue::VirtQueue>> {
        self.queues.lock().clone()
    }

    /// The queue at `index`, if activated.
    pub fn queue(&self, index: usize) -> Option<Arc<super::queue::VirtQueue>> {
        self.queues.lock().get(index).cloned()
    }

    fn read_status(&self) -> Result<u8> {
        self.common.read_val::<u8>(CFG_STATUS).map_err(|_| Errno::EIO)
    }

    fn set_status(&self, status: u8) -> Result<()> {
        self.common
            .write_val::<u8>(CFG_STATUS, &status)
            .map_err(|_| Errno::EIO)
    }

    fn write32(&self, offset: usize, value: u32) -> Result<()> {
        self.common
            .write_val::<u32>(offset, &value)
            .map_err(|_| Errno::EIO)
    }

    fn write16(&self, offset: usize, value: u16) -> Result<()> {
        self.common
            .write_val::<u16>(offset, &value)
            .map_err(|_| Errno::EIO)
    }

    fn read32(&self, offset: usize) -> Result<u32> {
        self.common.read_val::<u32>(offset).map_err(|_| Errno::EIO)
    }

    fn read16(&self, offset: usize) -> Result<u16> {
        self.common.read_val::<u16>(offset).map_err(|_| Errno::EIO)
    }

    /// Negotiated the features, brings the requested queues online and
    /// runs `driver.on_init`. Claimed by exactly one device.
    pub(super) fn activate(self: &Arc<Self>, driver: Arc<dyn VirtioDevice>) -> Result<()> {
        self.reset()?;
        // Common config sanity.
        if self.read32(CFG_MAGIC_VALUE)? != VIRTIO_MAGIC
            || self.read32(CFG_VERSION)? != VIRTIO_VERSION
        {
            crate::return_errno!(ENODEV, "device does not speak virtio 1.0");
        }

        self.set_status(STATUS_ACKNOWLEDGE)?;
        self.set_status(STATUS_DRIVER)?;
        // Mask MSIX (this device is driven polled).
        let _ = self.write16(CFG_MSIX_CONFIG, 0xFFFF);

        // Feature negotiation.
        self.write32(CFG_DEVICE_FEATURE_SEL, 0)?;
        let feature_lo = self.read32(CFG_DEVICE_FEATURE)?;
        self.write32(CFG_DEVICE_FEATURE_SEL, 1)?;
        let feature_hi = self.read32(CFG_DEVICE_FEATURE)?;
        let offered = feature_lo as u64 | (feature_hi as u64) << 32;
        if offered & VIRTIO_F_VERSION_1 == 0 {
            crate::return_errno!(ENODEV, "device lacks the version-1 feature");
        }
        let accepted = driver.accept_features(offered) | VIRTIO_F_VERSION_1;
        self.write32(CFG_DRIVER_FEATURE_SEL, 0)?;
        self.write32(CFG_DRIVER_FEATURE, accepted as u32)?;
        self.write32(CFG_DRIVER_FEATURE_SEL, 1)?;
        self.write32(CFG_DRIVER_FEATURE, (accepted >> 32) as u32)?;

        self.set_status(STATUS_FEATURES_OK)?;
        if self.read_status()? & STATUS_FEATURES_OK == 0 {
            self.set_status(STATUS_FAILED)?;
            crate::return_errno!(EIO, "device did not accept the feature set");
        }

        // Queues.
        let num_queues = self.read16(CFG_NUM_QUEUES)?;
        let count = driver.queue_count().min(num_queues as usize);
        for index in 0..count {
            self.write16(CFG_QUEUE_SELECT, index as u16)?;
            let size = self.read16(CFG_QUEUE_SIZE)?;
            if size == 0 {
                crate::return_errno!(EIO, "queue is zero-sized in common config");
            }
            let notify_off = self.read16(CFG_QUEUE_NOTIFY_OFF)?;

            let queue = super::queue::VirtQueue::new(
                index as u16,
                size,
                notify_off as usize,
                &Arc::downgrade(self),
            )?;
            let (desc, avail, used) = queue.ring_addresses();
            self.write32(CFG_QUEUE_DESC, desc as u32)?;
            self.write32(CFG_QUEUE_DESC_HI, (desc >> 32) as u32)?;
            self.write32(CFG_QUEUE_AVAIL, avail as u32)?;
            self.write32(CFG_QUEUE_AVAIL_HI, (avail >> 32) as u32)?;
            self.write32(CFG_QUEUE_USED, used as u32)?;
            self.write32(CFG_QUEUE_USED_HI, (used >> 32) as u32)?;
            let _ = self.write16(CFG_QUEUE_MSIX_VECTOR, 0xFFFF);
            self.write16(CFG_QUEUE_ENABLE, 1)?;

            self.queues.lock().push(queue);
        }

        self.set_status(STATUS_DRIVER_OK)?;
        if self.read_status()? & STATUS_DRIVER_OK == 0 {
            crate::return_errno!(EIO, "device refused DRIVER_OK");
        }

        driver.on_init(self)?;
        *self.driver.lock() = Some(driver);
        Ok(())
    }

    /// Reset with wait loop; the device ignores the write until idle.
    fn reset(&self) -> Result<()> {
        self.set_status(STATUS_RESET)?;
        let mut spins = 0;
        while self.read_status()? != STATUS_RESET {
            core::hint::spin_loop();
            spins += 1;
            if spins > 1_000_000 {
                crate::return_errno!(EIO, "device did not accept the reset");
            }
        }
        Ok(())
    }

    /// Rings the queue doorbell (`notify_off * multiplier` into the notify
    /// window, value = queue index).
    pub(super) fn ring_bell(&self, notify_off: usize, index: u16) -> Result<()> {
        self.notify
            .write_once(notify_off * self.notify_multiplier as usize, &index)
            .map_err(|_| Errno::EIO)
    }

    /// Marks the host PCI records the device as claimed by this driver.
    pub(crate) fn mark_bound(&self) {
        let mut devices = PCI_DEVICES.lock();
        if let Some(target) = devices.iter_mut().find(|d| d.address == self.device.address) {
            target.bound_driver = Some("virtio");
        }
    }
}

impl Drop for VirtioTransport {
    fn drop(&mut self) {
        // Resetting makes the device release the rings' memory as well.
        let _ = self.set_status(STATUS_RESET);
    }
}

/// All scanning-derived transports, in scan order.
static TRANSPORTS: Mutex<Vec<Arc<VirtioTransport>>> = Mutex::new(Vec::new());

/// The device drivers registered with the bus.
static DRIVERS: Mutex<Vec<Arc<dyn VirtioDevice>>> = Mutex::new(Vec::new());

/// Bring-up: scans the enumerated PCI list for modern virtio devices.
pub fn init() -> Result<()> {
    let candidates: Vec<PciDevice> = PCI_DEVICES
        .lock()
        .iter()
        .filter(|device| device.bound_driver.is_none())
        .cloned()
        .collect();

    for device in candidates {
        // RedHat vendor's modern device ids are `0x1040 + virtio type`.
        if device.vendor_id != 0x1AF4 || !(0x1040..0x1080).contains(&device.device_id) {
            continue;
        }
        match VirtioTransport::new(device) {
            Ok(transport) => {
                TRANSPORTS.lock().push(transport);
            }
            Err(err) => {
                ostd::error!("virtio: transport walk failed: {:?}", err);
            }
        }
    }
    bind_devices();
    Ok(())
}

/// Registers a device driver and lets it claim its transports.
pub fn register_virtio_device(device: Arc<dyn VirtioDevice>) -> Result<()> {
    DRIVERS.lock().push(Arc::clone(&device));
    for transport in TRANSPORTS.lock().iter() {
        if transport.driver.lock().is_some()
            || transport.pci_device_id() != device.device_id()
        {
            continue;
        }
        match transport.activate(Arc::clone(&device)) {
            Ok(()) => {
                ostd::info!(
                    "virtio: {} device (PCI {}) activated",
                    device.name(),
                    transport.pci_device_id()
                );
                transport.mark_bound();
            }
            Err(err) => {
                ostd::error!("virtio: {} activation failed: {:?}", device.name(), err);
            }
        }
    }
    Ok(())
}

/// Binds all registered drivers over the transport table (init path).
fn bind_devices() {
    let drivers = DRIVERS.lock().clone();
    for driver in drivers {
        let transports = TRANSPORTS.lock().clone();
        for transport in transports {
            if transport.driver.lock().is_some()
                || transport.pci_device_id() != driver.device_id()
            {
                continue;
            }
            if let Err(err) = transport.activate(Arc::clone(&driver)) {
                ostd::error!("virtio: {} activation failed: {:?}", driver.name(), err);
            } else {
                transport.mark_bound();
            }
        }
    }
}
