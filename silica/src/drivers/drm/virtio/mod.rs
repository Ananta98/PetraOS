// SPDX-License-Identifier: GPL-2.0

//! Virtio GPU 2D device driver (`virtio 1.2` §5.7).
//!
//! Structured into focused modules:
//! - [`protocol`]: 2D, cursor, and EDID wire command/response definitions,
//!   feature bits, pixel formats, and configuration space access.
//! - [`pipeline`]: 2D resource lifecycle management, damage rectangle clamping,
//!   `TransferOffset` handling (`QEMU` vs `crosvm`), and hardware cursor state.
//! - [`modes`]: EDID validation, preferred timing extraction, and virtual
//!   connector mode synthesis.
//! - [`device`]: [`VirtioGpu`] KMS/DRM driver implementation and VirtIO bus
//!   registration ([`VirtioGpuDevice`]).

pub mod device;
pub mod modes;
pub mod pipeline;
pub mod protocol;

use alloc::sync::Arc;

pub use device::{VirtioGpu, VirtioGpuDevice};
pub use protocol::PCI_DEVICE_ID_VIRTIO_GPU;

use crate::drivers::bus::virtio::register_virtio_device;
use crate::modules::InitcallLevel;
use crate::uapi::errno::Result;

/// Registers the `virtio-gpu` driver with the VirtIO bus.
pub fn init() -> Result<()> {
    register_virtio_device(Arc::new(VirtioGpuDevice))
}

crate::module!(
    "Virtio GPU 2D Driver",
    "Ananta98",
    InitcallLevel::Device,
    init
);
