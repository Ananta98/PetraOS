// SPDX-License-Identifier: GPL-2.0

//! Shared Zero-Page singleton for memory deduplication.
//!
//! Anonymous private mappings (`MAP_PRIVATE | MAP_ANONYMOUS`) read zeroes on their
//! first access. Rather than allocating and clearing a 4 KiB physical frame for
//! every read access, the kernel maps a single global read-only Zero Page.
//! When a write is performed on that page, the Copy-on-Write handler allocates a
//! private writable physical frame.

use ostd::mm::{HasPaddr, Paddr, UFrame};
use ostd::sync::SpinLock;

use super::alloc_zeroed_frame;
use crate::api::errno::Result;

static ZERO_FRAME: SpinLock<Option<UFrame>> = SpinLock::new(None);

/// Returns a clone of the global zero-filled frame.
///
/// The frame is allocated and zeroed once on first demand and reused across all
/// anonymous read faults in all processes.
pub fn get_zero_frame() -> Result<UFrame> {
    let mut lock = ZERO_FRAME.lock();
    if let Some(frame) = lock.as_ref() {
        return Ok(frame.clone());
    }
    let frame = alloc_zeroed_frame()?;
    *lock = Some(frame.clone());
    Ok(frame)
}

/// Returns whether `paddr` corresponds to the global zero frame.
pub fn is_zero_frame_paddr(paddr: Paddr) -> bool {
    let lock = ZERO_FRAME.lock();
    if let Some(frame) = lock.as_ref() {
        frame.paddr() == paddr
    } else {
        false
    }
}

/// Returns whether `frame` is the global zero frame.
pub fn is_zero_frame(frame: &UFrame) -> bool {
    is_zero_frame_paddr(frame.paddr())
}
