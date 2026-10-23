// SPDX-License-Identifier: GPL-2.0

//! DRM Display Subsystem, in the shape of the Linux KMS/GEM core.
//!
//! - [`core`]: the [`crate::drivers::drm::core::DrmDevice`] trait, per-fd
//!   [`crate::drivers::drm::core::DrmFile`] state and the shared ioctl
//!   dispatchers.
//! - [`gem`]: GEM buffer objects, per-file handle tables and user-accessible
//!   mappings.
//! - [`kms`]: mode-setting object tables and the 0xA0..0xB8 ioctl family.
//! - [`virtio`]: the virtio-gpu 2D driver (the QEMU test device).
//!
//! The legacy [`framebuffer`] fbdev stack stays until every in-tree user of
//! `/dev/fb0` is ported to the DRM ioctls.

pub mod core;
pub mod framebuffer;
pub mod gem;
pub mod kms;
pub mod virtio;

pub use core::{DrmDevice, DrmFile, register_drm_device};
pub(crate) use core::{read_uapi, write_uapi};
pub use framebuffer::get_framebuffer;
pub use gem::GemObject;
pub use kms::{DisplayDriver, Framebuffer, KmsPipeline, KmsState};
