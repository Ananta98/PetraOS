// SPDX-License-Identifier: GPL-2.0

//! Intel Integrated Graphics (`i915`) DRM/KMS/GEM driver.
//!
//! Structured into focused domain modules:
//! - [`regs`]: Hardware register offsets, bitfields, PCI IDs, and [`regs::IntelGen`]
//!   generation classification (`Gen7` through `Gen12+`).
//! - [`gtt`]: Global Graphics Translation Table ([`gtt::GgttManager`]) virtual
//!   address allocator and PTE programmer for [`crate::drivers::drm::GemObject`]s.
//! - [`display`]: Display pipe discovery, GMBUS DDC/EDID reading, Universal/Primary
//!   plane scanout programming, and hardware `64x64` cursor support.
//! - [`ioctl`]: `i915`-specific GEM and parameter ioctl handlers (`GETPARAM`,
//!   `GEM_CREATE`, `GEM_PREAD`, `GEM_PWRITE`, `GEM_MMAP_GTT`, `GEM_MMAP_OFFSET`,
//!   `GEM_SET_DOMAIN`, `GEM_SW_FINISH`, `GEM_SET_TILING`, `GEM_GET_TILING`).
//! - [`device`]: [`IntelGpu`] DRM/KMS device and [`IntelPciDriver`] PCI bus driver.

pub mod device;
pub mod display;
pub mod gtt;
pub mod ioctl;
pub mod regs;

use alloc::sync::Arc;

pub use device::{IntelGpu, IntelPciDriver};
pub use regs::{IntelGen, PCI_VENDOR_ID_INTEL};

use crate::drivers::bus::pci::pci_register_driver;
use crate::modules::InitcallLevel;
use crate::uapi::errno::Result;

/// Registers the Intel `i915` PCI driver with the PCI bus.
pub fn init() -> Result<()> {
    pci_register_driver(Arc::new(IntelPciDriver))
}

crate::module!(
    "Intel i915 DRM Driver",
    "Ananta98",
    InitcallLevel::Device,
    init
);
