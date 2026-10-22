// SPDX-License-Identifier: GPL-2.0

//! Virtio transport (`virtio-pci`, modern devices), a standalone module that
//! is a neighbour of `bus/pci` in [`crate::drivers::bus::virtio`].
//!
//! The transport owns the bus side of a modern virtio 1.0 PCI device:
//! capability discovery, feature negotiation, queue bring-up and MMIO
//! notification. Device drivers ([`VirtioDevice`]) register with the bus,
//! which scans the enumerated PCI list and claims each matching unclaimed
//! device.

pub mod caps;
pub mod queue;
pub mod transport;

pub use caps::{VirtioCap, VirtioCapType};
pub use queue::{SubmitBuffer, VirtQueue};
pub use transport::{VirtioDevice, VirtioTransport, register_virtio_device};

/// Scans the enumerated PCI devices for modern virtio ones and binds the
/// registered drivers (runs before `Device`-level GPU drivers register).
pub fn init() -> crate::uapi::errno::Result<()> {
    transport::init()
}

crate::module!(
    "Virtio PCI Transport",
    "Ananta98",
    crate::modules::InitcallLevel::Subsys,
    init
);
