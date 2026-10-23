// SPDX-License-Identifier: GPL-2.0

//! DRM core: card registry, per-file state and the ioctl dispatcher.
//!
//! The core is the shared glue behind every DRM driver: it owns the
//! `/dev/dri/cardN` character nodes (exposed through devfs),
//! the GEM handle/offset bookkeeping (see [`crate::drivers::drm::gem`]), and
//! the translation of Linux-ABI ioctl requests into driver callbacks of
//! [`DrmDevice`].
//!
//! The subsystem starts up empty — GPU drivers populate the registry during
//! their own device initcalls.

pub mod device;
pub mod file;
pub mod ioctl;

pub use device::{DRM_DEVICES, DrmDevice, get_device, register_drm_device};
pub use file::{ClientCaps, DrmFile};
pub(crate) use ioctl::{read_uapi, write_uapi};

/// Initializes the DRM core; a no-op for now, present so the module! entry
/// exists and ordering of later GPU drivers is defined.
pub fn init() -> crate::uapi::errno::Result<()> {
    ostd::info!("DRM core initialized");
    Ok(())
}
