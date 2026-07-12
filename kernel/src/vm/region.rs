use ostd::mm::{PageFlags, Vaddr};

pub struct VmaRegion {
    pub start: Vaddr,
    pub size: usize,
    pub flags: PageFlags,
    /// Number of guard pages at the bottom (lowest address) of the region.
    /// Guard pages are never mapped; any access triggers a fault error.
    pub guard_size: usize,
}

impl VmaRegion {
    pub fn new(start: Vaddr, size: usize, flags: PageFlags) -> Self {
        Self {
            start,
            size,
            flags,
            guard_size: 0,
        }
    }

    pub fn new_with_guard(start: Vaddr, size: usize, flags: PageFlags, guard_size: usize) -> Self {
        Self {
            start,
            size,
            flags,
            guard_size,
        }
    }

    pub fn contains(&self, addr: Vaddr) -> bool {
        self.start
            .checked_add(self.size)
            .map_or(false, |end| addr >= self.start && addr < end)
    }

    pub fn size(&self) -> usize {
        self.size
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
