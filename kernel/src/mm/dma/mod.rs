//! Direct Memory Access (DMA) allocation subsystem.
//!
//! PetraOS runs on x86-64 with a 1:1 higher-half direct map (HHDM), so any
//! physical page is already accessible from the kernel at `phys + hhdm_offset`.
//! The allocators below build on that by providing DMA-safe bookkeeping on top
//! of the physical memory manager (`PMM`):
//!
//! * [`DmaCoherent`] — long-lived, physically-contiguous, cache-coherent buffers
//!   mapped into the kernel address space. Suitable for structures the device and
//!   CPU share for the lifetime of the driver (e.g. submission/completion queues,
//!   identify response buffers).
//! * [`DmaStreamer`] — short-lived streaming mappings that bounce data between a
//!   caller buffer and a device-accessible region for a single transfer. This
//!   abstracts away non-contiguous caller buffers and cache maintenance.

mod coherent;
mod streamer;

pub use coherent::DmaCoherent;
pub use streamer::{DmaDirection, DmaStreamer};

/// Errors returned by the DMA allocation subsystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DmaError {
    /// The physical memory manager could not satisfy the request.
    OutOfMemory,
    /// A zero-sized allocation was requested.
    InvalidSize,
}
