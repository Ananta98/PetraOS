/// VirtIO PCI Transport Module
///
/// Provides a reusable PCI transport abstraction for VirtIO devices,
/// supporting both the legacy (transitional) and modern (VirtIO 1.0+)
/// transport variants.
///
/// # Usage
///
/// ```rust,ignore
/// use crate::drivers::bus::virtio_pci::VirtioPciTransport;
/// use crate::drivers::bus::pci::enumerate;
/// use crate::drivers::bus::virtio_pci::regs;
///
/// for pci_dev in enumerate() {
///     if pci_dev.vendor_id != regs::VIRTIO_VENDOR_ID { continue; }
///
///     let transport = VirtioPciTransport::probe(pci_dev)?;
///     transport.reset()?;
///     transport.add_status(regs::STATUS_ACKNOWLEDGE)?;
///     transport.add_status(regs::STATUS_DRIVER)?;
///
///     let features = transport.read_device_features()?;
///     transport.write_driver_features(features & SUPPORTED_FEATURES)?;
///
///     transport.add_status(regs::STATUS_FEATURES_OK)?;
///     // … configure queues …
///     transport.add_status(regs::STATUS_DRIVER_OK)?;
/// }
/// ```
///
/// # Module layout
///
/// | File | Responsibility |
/// |------|---------------|
/// | [`regs`]       | Device-agnostic VirtIO constants (status bits, offsets, feature flags) |
/// | [`capability`] | VirtIO PCI vendor capability parsing (`VIRTIO_PCI_CAP_*`) |
/// | [`transport`]  | [`VirtioPciTransport`] — unified legacy/modern transport API |
/// | [`virtqueue`]  | [`SplitVirtqueue`] — shared split virtqueue for all VirtIO device drivers |
mod capability;
pub mod regs;
mod transport;
pub mod virtqueue;

pub use capability::{VirtioPciCapability, find_cap, parse_virtio_capabilities};
pub use transport::{TransportKind, VirtioPciTransport};
pub use virtqueue::{SplitVirtqueue, VirtqDescFlags, VirtqDescriptor};
