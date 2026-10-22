// SPDX-License-Identifier: GPL-2.0

//! User-space mapping support for GEM objects (`mmap(2)`).

use alloc::sync::Arc;
use core::fmt::{self, Debug, Formatter};
use core::ops::Range;
use ostd::mm::{PAGE_SIZE, UFrame};

use super::object::GemObject;
use crate::uapi::errno::Result;
use crate::vm::backing::Backing;

/// Mapping backing of a [`GemObject`].
///
/// The object owns the resident-page cache (`GemObject::cached_frame`) and its
/// writeback (`GemObject::flush_pages`); this type just translates mapping
/// addresses into object page offsets. All mappings of one object share the
/// same cache, so writes performed through any of them are visible to the
/// others and are staged into the object's DMA memory by [`Backing::flush`].
pub struct GemBacking {
    object: Arc<GemObject>,
    /// Mapping-space offset of the object's byte 0: `0` for whole-object
    /// backings, the GEM fake-offset grant for `/dev/dri` mmaps.
    base: usize,
    /// Page-aligned size of the object in bytes.
    size: usize,
}

impl GemBacking {
    /// Creates a backing exposing the object at base offset zero.
    pub fn new(object: Arc<GemObject>) -> Arc<Self> {
        Arc::new(Self {
            size: object.size(),
            base: 0,
            object,
        })
    }

    /// Creates the backing of an `mmap(2)` at `mapping_offset` on a DRM card:
    /// the object's first byte sits at that offset in the mapping.
    pub fn granted(object: Arc<GemObject>, mapping_offset: u64) -> Arc<Self> {
        Arc::new(Self {
            size: object.size(),
            base: mapping_offset as usize,
            object,
        })
    }

    /// Creates a base-zero backing and materializes the whole object, which
    /// is useful when a page was prepared before being mapped.
    pub fn new_prepopulated(object: Arc<GemObject>) -> Result<Arc<Self>> {
        let backing = Self::new(Arc::clone(&object));
        for offset in (0..backing.size).step_by(PAGE_SIZE) {
            backing.frame(offset)?;
        }
        Ok(backing)
    }
}

impl Backing for GemBacking {
    fn size(&self) -> usize {
        self.size
    }

    fn frame(&self, offset: usize) -> Result<UFrame> {
        if offset < self.base || (offset - self.base) % PAGE_SIZE != 0 {
            crate::return_errno!(EINVAL, "page offset must be page-aligned");
        }
        self.object.cached_frame(offset - self.base)
    }

    fn flush(&self, range: Range<usize>) -> Result<()> {
        // Intersect with the object's span in mapping space.
        let start = range.start.max(self.base);
        let end = range.end.min(self.base + self.size);
        if start < end {
            self.object.flush_pages(start - self.base..end - self.base);
        }
        Ok(())
    }
}

impl Debug for GemBacking {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("GemBacking")
            .field("object", &self.object)
            .field("base", &self.base)
            .field("size", &self.size)
            .finish()
    }
}
