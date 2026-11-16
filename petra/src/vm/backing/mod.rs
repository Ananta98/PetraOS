// SPDX-License-Identifier: GPL-2.0

//! Physical memory backing and frame allocation.

pub mod file;
pub mod page_cache;
pub mod shm;
pub mod zero;

use core::{fmt::Debug, ops::Range, sync::atomic::{AtomicUsize, Ordering}};
use ostd::mm::{FrameAllocOptions, UFrame};

use crate::uapi::errno::Result;

pub use file::FileBacking;
pub use shm::SharedPages;
pub use zero::*;

/// A source of physical pages for a mapping.
pub trait Backing: Send + Sync + Debug {
    /// Returns the size of the object in bytes.
    fn size(&self) -> usize;

    /// Returns the page that backs `offset`, materializing it if needed.
    fn frame(&self, offset: usize) -> Result<UFrame>;

    /// Makes every change to `range` visible to storage or other mappings.
    fn flush(&self, _range: Range<usize>) -> Result<()> {
        Ok(())
    }
}

static ALLOC_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Allocates a single zero-filled page from the physical memory allocator.
/// Periodically wakes the background reclaim daemon and falls back to synchronous
/// reclaim if physical memory allocation fails.
pub fn alloc_zeroed_frame() -> Result<UFrame> {
    if ALLOC_COUNTER.fetch_add(1, Ordering::Relaxed) % 2048 == 2047 {
        crate::vm::reclaim::wake_reclaim_daemon();
    }
    match FrameAllocOptions::new().alloc_frame() {
        Ok(frame) => Ok(frame.into()),
        Err(err) => {
            if crate::vm::reclaim::reclaim_at_least(64) > 0 {
                Ok(FrameAllocOptions::new().alloc_frame()?.into())
            } else {
                Err(err.into())
            }
        }
    }
}
