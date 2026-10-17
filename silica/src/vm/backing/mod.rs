// SPDX-License-Identifier: GPL-2.0

//! Physical memory backing and frame allocation.

pub mod file;
pub mod shm;
pub mod zero;

use core::{fmt::Debug, ops::Range};
use ostd::mm::{FrameAllocOptions, UFrame};

use crate::api::errno::Result;

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

/// Allocates a single zero-filled page from the physical memory allocator.
pub fn alloc_zeroed_frame() -> Result<UFrame> {
    Ok(FrameAllocOptions::new().alloc_frame()?.into())
}
