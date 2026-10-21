// SPDX-License-Identifier: GPL-2.0

//! Kernel Mode Setting (KMS): the display pipeline layer of DRM.
//!
//! Mode-setting is driver-agnostic: a driver describes its pipelines
//! (`KmsPipeline`) and how to program a scanout ([`DisplayDriver`]), and the
//! generic ioctl layer in `ioctl.rs` maps the Linux KMS request family onto
//! that state. Framebuffer object tables live in [`KmsState`].

pub mod ioctl;
pub mod state;

pub use state::{DisplayDriver, Framebuffer, KmsPipeline, KmsState};
