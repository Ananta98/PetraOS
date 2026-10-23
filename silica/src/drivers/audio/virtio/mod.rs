// SPDX-License-Identifier: GPL-2.0

//! Virtio sound device driver (`virtio 1.2` §5.14).
//!
//! Structured into focused modules:
//! - [`protocol`]: PCI device IDs, virtqueue indices, `virtio_snd_config`
//!   layout, PCM stream control opcodes, format/rate tables, and wire headers.
//! - [`device`]: [`VirtioSnd`] [`AudioDevice`](crate::drivers::AudioDevice)
//!   implementation and VirtIO bus registration ([`VirtioSndDriver`]).

pub mod device;
pub mod protocol;

use alloc::sync::Arc;

pub use device::{VirtioSnd, VirtioSndDriver};
pub use protocol::PCI_DEVICE_ID_VIRTIO_SND;

use crate::drivers::bus::virtio::register_virtio_device;
use crate::modules::InitcallLevel;
use crate::uapi::errno::Result;

/// Registers the `virtio-snd` driver with the VirtIO bus.
pub fn init() -> Result<()> {
    register_virtio_device(Arc::new(VirtioSndDriver))
}

crate::module!(
    "Virtio Sound Driver",
    "Ananta",
    InitcallLevel::Device,
    init
);
