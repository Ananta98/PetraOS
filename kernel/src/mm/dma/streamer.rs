//! Streaming DMA mapping with bounce buffering.
//!
//! A [`DmaStreamer`] provides a single-transfer DMA mapping for a caller
//! buffer. Because the caller's buffer may be virtual-only, non-contiguous, or
//! otherwise unsuitable for device access, the streamer owns a [`DmaCoherent`]
//! bounce buffer that the device can address, and copies data in/out as
//! required by the transfer direction.
//!
//! Usage:
//! * Write (CPU -> device): `sync_for_device(src)` then let the device DMA from
//!   [`DmaStreamer::phys`], then drop.
//! * Read (device -> CPU): let the device DMA into [`DmaStreamer::phys`], then
//!   `sync_for_cpu(dst)`.

use super::coherent::DmaCoherent;
use super::DmaError;
use crate::mm::{PhysAddr, VirtAddr};

/// Direction of data flow for a streaming DMA mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DmaDirection {
    /// Data flows from CPU memory to the device (a write request).
    ToDevice,
    /// Data flows from the device into CPU memory (a read completion).
    FromDevice,
    /// Data may flow in either direction during the transfer.
    Bidirectional,
}

/// A short-lived streaming DMA mapping backed by a bounce buffer.
///
/// The bounce buffer is freed when this value is dropped.
pub struct DmaStreamer {
    bounce: DmaCoherent,
    len: usize,
    direction: DmaDirection,
}

impl DmaStreamer {
    /// Allocate a streaming mapping of `size` bytes for the given direction.
    pub fn new(size: usize, direction: DmaDirection) -> Result<Self, DmaError> {
        if size == 0 {
            return Err(DmaError::InvalidSize);
        }
        let bounce = DmaCoherent::alloc(size)?;
        Ok(Self {
            bounce,
            len: size,
            direction,
        })
    }

    /// The device-visible physical address of the bounce buffer.
    pub fn phys(&self) -> PhysAddr {
        self.bounce.phys()
    }

    /// The kernel virtual address of the bounce buffer.
    pub fn virt(&self) -> VirtAddr {
        self.bounce.virt()
    }

    /// Number of bytes covered by this mapping.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether this mapping covers zero bytes.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Transfer direction declared at allocation time.
    pub fn direction(&self) -> DmaDirection {
        self.direction
    }

    /// Copy `src` into the bounce buffer prior to a `ToDevice` transfer.
    ///
    /// Only the first `min(src.len(), len)` bytes are copied.
    pub fn sync_for_device(&mut self, src: &[u8]) {
        let n = core::cmp::min(src.len(), self.len);
        // SAFETY: `n` is bounded by the allocated bounce buffer size.
        unsafe {
            core::ptr::copy_nonoverlapping(src.as_ptr(), self.bounce.as_mut_ptr(), n);
        }
    }

    /// Copy the bounce buffer into `dst` after a `FromDevice` transfer.
    ///
    /// Only the first `min(dst.len(), len)` bytes are copied.
    pub fn sync_for_cpu(&self, dst: &mut [u8]) {
        let n = core::cmp::min(dst.len(), self.len);
        // SAFETY: `n` is bounded by the allocated bounce buffer size.
        unsafe {
            core::ptr::copy_nonoverlapping(self.bounce.as_ptr(), dst.as_mut_ptr(), n);
        }
    }
}
