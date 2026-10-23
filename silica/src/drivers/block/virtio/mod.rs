// SPDX-License-Identifier: GPL-2.0

//! Virtio block device driver (`virtio 1.2` §5.2).
//!
//! Structured into focused modules:
//! - [`protocol`]: PCI device IDs, feature bits, `virtio_blk_config` layout,
//!   request headers, opcodes, and completion status codes.
//! - [`device`]: [`VirtioBlk`] [`BlockDevice`](crate::drivers::BlockDevice)
//!   implementation and VirtIO bus registration ([`VirtioBlkDriver`]).

pub mod device;
pub mod protocol;

use alloc::sync::Arc;

pub use device::{VirtioBlk, VirtioBlkDriver};
pub use protocol::PCI_DEVICE_ID_VIRTIO_BLK;

use crate::drivers::bus::virtio::register_virtio_device;
use crate::modules::InitcallLevel;
use crate::uapi::errno::Result;

/// Registers the `virtio-blk` driver with the VirtIO bus.
pub fn init() -> Result<()> {
    register_virtio_device(Arc::new(VirtioBlkDriver))
}

crate::module!(
    "Virtio Block Storage Driver",
    "Ananta",
    InitcallLevel::Device,
    init
);
