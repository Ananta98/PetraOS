// SPDX-License-Identifier: GPL-2.0

//! Virtio GPU 2D device driver.
//!
//! Attaches a modern virtio-gpu PCI device to the DRM stack: dumb buffers
//! become 2D resources backed by their GEM object, `SETCRTC`/`PAGE_FLIP`
//! program a scanout with `[SET_SCANOUT, RESOURCE_FLUSH]`, and `DIRTYFB`
//! triggers a `[TRANSFER_TO_HOST_2D, RESOURCE_FLUSH]` refresh so user-space
//! drawing becomes visible.

pub mod device;

pub use device::VirtioGpuDevice;

/// virtio-gpu over PCI: `VIRTIO_ID_GPU = 16`, modern ids `0x1040 + type`.
pub const PCI_DEVICE_ID_VIRTIO_GPU: u16 = 0x1050;

/// Registers the driver with the virtio bus, which binds it to any
/// unclaimed matching PCI device.
pub fn init() -> crate::api::errno::Result<()> {
    crate::drivers::bus::virtio::register_virtio_device(alloc::sync::Arc::new(VirtioGpuDevice))
}

crate::module!(
    "Virtio GPU 2D Driver",
    "Ananta98",
    crate::modules::InitcallLevel::Device,
    init
);
