// SPDX-License-Identifier: GPL-2.0

//! Virtio network device driver (`virtio 1.2` §5.1).
//!
//! Structured into focused modules:
//! - [`protocol`]: PCI device IDs, virtqueue indices, feature bits,
//!   `virtio_net_config` layout, and the `virtio_net_hdr` packet header.
//! - [`device`]: [`VirtioNet`] [`NetDevice`](crate::drivers::NetDevice)
//!   implementation and VirtIO bus registration ([`VirtioNetDriver`]).

pub mod device;
pub mod protocol;

use alloc::sync::Arc;

pub use device::{VirtioNet, VirtioNetDriver};
pub use protocol::PCI_DEVICE_ID_VIRTIO_NET;

use crate::drivers::bus::virtio::register_virtio_device;
use crate::modules::InitcallLevel;
use crate::uapi::errno::Result;

/// Registers the `virtio-net` driver with the VirtIO bus.
pub fn init() -> Result<()> {
    register_virtio_device(Arc::new(VirtioNetDriver))
}

crate::module!(
    "Virtio Network Driver",
    "Ananta",
    InitcallLevel::Device,
    init
);
