use alloc::sync::Arc;
use ostd::Error;
use ostd::mm::{PageFlags, Vaddr};

use crate::fs::vfs::FileOps;
use ostd::sync::SpinLock;

/// A virtual memory area descriptor representing a contiguous region in
/// a process's address space.
#[derive(Clone)]
pub struct VmaRegion {
    pub start: Vaddr,
    pub size: usize,
    pub flags: PageFlags,
    /// Number of guard pages at the bottom (lowest address) of the region.
    /// Guard pages are never mapped; any access triggers a fault error.
    pub guard_size: usize,
    /// Optional file backing for file-backed mappings.
    pub file_backing: Option<Arc<SpinLock<dyn FileOps>>>,
    /// Offset within the backing file where this mapping starts.
    pub file_offset: usize,
    /// Whether this is a MAP_SHARED mapping.
    pub is_shared: bool,
}

impl VmaRegion {
    /// Creates a new anonymous VMA region with the given start address, size,
    /// and page flags. Guard pages, file backing, and sharing are unset.
    pub fn new(start: Vaddr, size: usize, flags: PageFlags) -> Self {
        Self {
            start,
            size,
            flags,
            guard_size: 0,
            file_backing: None,
            file_offset: 0,
            is_shared: false,
        }
    }

    /// Creates a new file-backed VMA region with the given backing source,
    /// file offset, and sharing mode.
    pub fn new_file_backed(
        start: Vaddr,
        size: usize,
        flags: PageFlags,
        file_backing: Arc<SpinLock<dyn FileOps>>,
        file_offset: usize,
        is_shared: bool,
    ) -> Self {
        Self {
            start,
            size,
            flags,
            guard_size: 0,
            file_backing: Some(file_backing),
            file_offset,
            is_shared,
        }
    }

    /// Returns whether the given address falls within this region.
    pub fn contains(&self, addr: Vaddr) -> bool {
        self.start
            .checked_add(self.size)
            .map_or(false, |end| addr >= self.start && addr < end)
    }
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;

    #[ktest]
    fn test_vma_region_contains() {
        let region = VmaRegion::new(0x1000, 0x2000, PageFlags::empty());
        assert!(region.contains(0x1000));
        assert!(region.contains(0x2000));
        assert!(region.contains(0x2fff));
        assert!(!region.contains(0xfff));
        assert!(!region.contains(0x3000));
    }
}
