use alloc::sync::Arc;
use ostd::Error;
use ostd::mm::{PageFlags, Vaddr};

use crate::fs::vfs::FileOps;
use ostd::sync::SpinLock;

/// A virtual memory area (VMA) descriptor representing a contiguous range of
/// virtual addresses within a process's address space.
///
/// VMAs are used to track memory layout, permissions, page-fault handling behavior
/// (such as demand paging or copy-on-write), and backing stores (like files or IPC shared memory).
#[derive(Clone)]
pub struct VmaRegion {
    /// The starting virtual address of this region.
    /// Must be page-aligned to facilitate direct hardware mapping.
    pub start: Vaddr,
    /// The size of the region in bytes.
    /// Must be a multiple of `PAGE_SIZE`.
    pub size: usize,
    /// Memory protection flags (Read, Write, Execute) applied to pages in this region.
    pub flags: PageFlags,
    /// Optional backing file representation for file-backed memory mappings.
    ///
    /// If `Some`, page faults in this region will load data from the file on demand.
    pub file_backing: Option<Arc<SpinLock<dyn FileOps>>>,
    /// The byte offset within the backing file from which the mapping starts.
    ///
    /// This allows mapping specific portions of a file (e.g., ELF sections) to arbitrary
    /// virtual memory locations.
    pub file_offset: usize,
    /// Indicates whether modifications to pages in this region should be shared with
    /// other processes mapping the same underlying memory/file.
    ///
    /// If true, writes are visible to other processes and written back to the file.
    /// If false, writes are handled via copy-on-write (COW) mechanisms.
    pub is_shared: bool,
}

impl VmaRegion {
    /// Creates a new anonymous VMA region with the given start address, size,
    /// and page flags.
    ///
    /// By default, the region is not file-backed and is private (not shared).
    /// Used for standard heap allocations, BSS, or basic data segments.
    pub fn new(start: Vaddr, size: usize, flags: PageFlags) -> Self {
        Self {
            start,
            size,
            flags,
            file_backing: None,
            file_offset: 0,
            is_shared: false,
        }
    }

    /// Creates a new file-backed VMA region with the given backing source,
    /// file offset, and sharing mode.
    ///
    /// File-backed VMAs read content on demand (demand paging) from the underlying
    /// file representation. If `is_shared` is true, writes propagate back to the file
    /// or shared mappings.
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
            file_backing: Some(file_backing),
            file_offset,
            is_shared,
        }
    }

    /// Returns the end address (exclusive) of this region.
    pub fn end(&self) -> Vaddr {
        self.start + self.size
    }

    /// Returns whether the given address falls within the bounds of this region.
    ///
    /// Includes the `start` address and excludes the end address (`start + size`).
    pub fn contains(&self, addr: Vaddr) -> bool {
        self.start
            .checked_add(self.size)
            .map_or(false, |end| addr >= self.start && addr < end)
    }

    /// Splits this region into two sub-regions at `split_addr`.
    ///
    /// Preserves file-backing, flags, and calculates the appropriate file offset.
    /// `split_addr` must lie strictly within `(self.start, self.end())`.
    pub fn split_at(&self, split_addr: Vaddr) -> (Self, Self) {
        let left_size = split_addr - self.start;
        let right_size = self.size - left_size;

        let left = Self {
            start: self.start,
            size: left_size,
            flags: self.flags,
            file_backing: self.file_backing.clone(),
            file_offset: self.file_offset,
            is_shared: self.is_shared,
        };

        let right = Self {
            start: split_addr,
            size: right_size,
            flags: self.flags,
            file_backing: self.file_backing.clone(),
            file_offset: self.file_offset + left_size,
            is_shared: self.is_shared,
        };

        (left, right)
    }

    /// Checks if this region can be merged with `other` (which must immediately follow `self`).
    pub fn can_merge_with(&self, other: &Self) -> bool {
        if self.end() != other.start {
            return false;
        }
        if self.flags != other.flags || self.is_shared != other.is_shared {
            return false;
        }
        match (&self.file_backing, &other.file_backing) {
            (None, None) => true,
            (Some(fb1), Some(fb2)) => {
                Arc::ptr_eq(fb1, fb2) && (self.file_offset + self.size == other.file_offset)
            }
            _ => false,
        }
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

    #[ktest]
    fn test_vma_region_split_at() {
        let region = VmaRegion::new(0x1000, 0x3000, PageFlags::RW);
        let (left, right) = region.split_at(0x2000);
        assert_eq!(left.start, 0x1000);
        assert_eq!(left.size, 0x1000);
        assert_eq!(right.start, 0x2000);
        assert_eq!(right.size, 0x2000);
    }

    #[ktest]
    fn test_vma_region_can_merge_with() {
        let r1 = VmaRegion::new(0x1000, 0x1000, PageFlags::RW);
        let r2 = VmaRegion::new(0x2000, 0x1000, PageFlags::RW);
        let r3 = VmaRegion::new(0x2000, 0x1000, PageFlags::R);
        assert!(r1.can_merge_with(&r2));
        assert!(!r1.can_merge_with(&r3));
    }
}
