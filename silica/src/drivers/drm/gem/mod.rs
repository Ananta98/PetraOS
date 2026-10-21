// SPDX-License-Identifier: GPL-2.0

//! Graphics Execution Manager (GEM).
//!
//! GEM is the DRM memory-management layer: it hands out [`GemObject`]s
//! (kernel-private units of video memory), tracks user-space references to
//! them per DRM file ([`GemHandleTable`]), and makes them mappable via
//! [`GemBacking`]. buffer objects back scanout framebuffers and any other
//! GPU-visible allocation.
//!
//! Should grow into the flagged object lifetime, DMA-buf export/import and
//! per-engine command buffers as the GPU drivers mature.

pub mod backing;
pub mod handle;
pub mod object;

pub use backing::GemBacking;
pub use handle::{FAKE_OFFSET_BASE, GemHandleTable, GemOffsetTable};
pub use object::GemObject;
