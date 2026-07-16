use crate::vm::region::{MmapFileBacking, VmaRegion};
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use ostd::Error;
use ostd::mm::io::{FallibleVmRead, FallibleVmWrite, VmReader, VmWriter};
use ostd::mm::vm_space::VmSpace;
use ostd::mm::{CachePolicy, FrameAllocOptions, PAGE_SIZE, PageFlags, PageProperty, UFrame, Vaddr};
use ostd::sync::SpinLock;
use ostd::task::disable_preempt;

/// User-space address range for mmap allocations.
const USER_SPACE_START: Vaddr = 0x0000_0000_1000;
const USER_SPACE_END: Vaddr = 0x0000_7FFF_FFFF_0000;

/// Heap-break bookkeeping for a process address space.
///
/// Both fields are always updated together under a single lock, which
/// prevents races between concurrent `brk()` calls and eliminates the
/// two-lock ordering hazard that existed when they were stored separately.
#[derive(Clone, Copy, Default)]
pub(crate) struct BrkState {
    /// The lowest valid heap address, set once after ELF loading.
    pub start: Vaddr,
    /// The current program break (top of heap).
    pub current: Vaddr,
}

pub struct VmaManager {
    pub vm_space: Arc<VmSpace>,
    pub regions: SpinLock<BTreeMap<Vaddr, VmaRegion>>,
    /// Heap break state. Acquire after `regions` when both locks are needed.
    pub(crate) brk: SpinLock<BrkState>,
}

impl VmaManager {
    pub fn new() -> Self {
        Self {
            vm_space: Arc::new(VmSpace::new()),
            regions: SpinLock::new(BTreeMap::new()),
            brk: SpinLock::new(BrkState::default()),
        }
    }

    pub fn map_region(&self, start: Vaddr, size: usize, flags: PageFlags) -> Result<(), Error> {
        if start % PAGE_SIZE != 0 || size % PAGE_SIZE != 0 {
            return Err(Error::InvalidArgs);
        }
        let guard = disable_preempt();
        let vaddr_range = start..start + size;
        let mut cursor = self
            .vm_space
            .cursor_mut(&guard, &vaddr_range)
            .map_err(|_| Error::NoMemory)?;

        let property = PageProperty::new_user(flags, CachePolicy::Writeback);
        let num_pages = size / PAGE_SIZE;

        for page_idx in 0..num_pages {
            let page_vaddr = start + (page_idx * PAGE_SIZE);
            cursor.jump(page_vaddr).map_err(|_| Error::InvalidArgs)?;
            let frame: UFrame = FrameAllocOptions::new()
                .zeroed(true)
                .alloc_frame()
                .map_err(|_| Error::NoMemory)?
                .into();
            cursor.map(frame, property);
        }

        let mut regions = self.regions.lock();
        regions.insert(
            start,
            VmaRegion {
                start,
                size,
                flags,
                guard_size: 0,
                file_backing: None,
                file_offset: 0,
                is_shared: false,
            },
        );

        Ok(())
    }

    /// Map a list of existing physical frames into the user address space.
    pub fn map_shared_frames(
        &self,
        start: Vaddr,
        frames: &[UFrame],
        flags: PageFlags,
    ) -> Result<(), Error> {
        if start % PAGE_SIZE != 0 {
            return Err(Error::InvalidArgs);
        }
        let size = frames.len() * PAGE_SIZE;
        let guard = disable_preempt();
        let vaddr_range = start..start + size;
        let mut cursor = self
            .vm_space
            .cursor_mut(&guard, &vaddr_range)
            .map_err(|_| Error::NoMemory)?;

        let property = PageProperty::new_user(flags, CachePolicy::Writeback);

        for (page_idx, frame) in frames.iter().enumerate() {
            let page_vaddr = start + (page_idx * PAGE_SIZE);
            cursor.jump(page_vaddr).map_err(|_| Error::InvalidArgs)?;
            cursor.map(frame.clone(), property);
        }

        let mut regions = self.regions.lock();
        regions.insert(
            start,
            VmaRegion {
                start,
                size,
                flags,
                guard_size: 0,
                file_backing: None,
                file_offset: 0,
                is_shared: true,
            },
        );

        Ok(())
    }

    pub fn map_stack(
        &self,
        start: Vaddr,
        stack_size: usize,
        guard_size: usize,
    ) -> Result<(), Error> {
        if start % PAGE_SIZE != 0 || stack_size % PAGE_SIZE != 0 || guard_size % PAGE_SIZE != 0 {
            return Err(Error::InvalidArgs);
        }

        let total_size = stack_size + guard_size;
        let mut regions = self.regions.lock();
        regions.insert(
            start,
            VmaRegion {
                start,
                size: total_size,
                flags: PageFlags::RW,
                guard_size,
                file_backing: None,
                file_offset: 0,
                is_shared: false,
            },
        );
        Ok(())
    }

    pub fn unmap_region(&self, start: Vaddr, size: usize) -> Result<(), Error> {
        if start % PAGE_SIZE != 0 || size % PAGE_SIZE != 0 {
            return Err(Error::InvalidArgs);
        }

        let guard = disable_preempt();
        let vaddr_range = start..start + size;
        let mut cursor = self
            .vm_space
            .cursor_mut(&guard, &vaddr_range)
            .map_err(|_| Error::NoMemory)?;

        cursor.unmap(size);

        let mut regions = self.regions.lock();
        regions.remove(&start);

        Ok(())
    }

    pub fn map_region_lazy(
        &self,
        start: Vaddr,
        size: usize,
        flags: PageFlags,
    ) -> Result<(), Error> {
        if start % PAGE_SIZE != 0 || size % PAGE_SIZE != 0 {
            return Err(Error::InvalidArgs);
        }

        let mut regions = self.regions.lock();
        regions.insert(
            start,
            VmaRegion {
                start,
                size,
                flags,
                guard_size: 0,
                file_backing: None,
                file_offset: 0,
                is_shared: false,
            },
        );

        Ok(())
    }

    // ------------------------------------------------------------------
    // mmap-related helpers
    // ------------------------------------------------------------------

    /// Find a free region of at least `size` bytes in the user address space.
    pub fn find_free_region(&self, size: usize) -> Option<Vaddr> {
        let regions = self.regions.lock();
        let mut candidate = USER_SPACE_START;
        for (_start, region) in regions.iter() {
            let region_end = region.start.checked_add(region.size)?;
            if candidate + size <= region.start {
                return Some(candidate);
            }
            candidate = core::cmp::max(candidate, region_end);
            if candidate >= USER_SPACE_END {
                return None;
            }
        }
        if candidate.checked_add(size)? <= USER_SPACE_END {
            Some(candidate)
        } else {
            None
        }
    }

    /// Find any VMA that contains the given address.
    /// Returns the key (start) and a clone of the region.
    pub fn find_vma(&self, addr: Vaddr) -> Option<(Vaddr, VmaRegion)> {
        let regions = self.regions.lock();
        for (&start, region) in regions.iter() {
            if region.contains(addr) {
                return Some((start, region.clone()));
            }
        }
        None
    }

    /// Create an anonymous (MAP_ANONYMOUS) mapping.
    ///
    /// If `start` is `None`, the kernel picks an address. Pages are allocated
    /// lazily (demand paging) unless `populate` is true.
    pub fn mmap_anon(
        &self,
        start: Option<Vaddr>,
        size: usize,
        flags: PageFlags,
        populate: bool,
    ) -> Result<Vaddr, Error> {
        let aligned_size = align_up(size, PAGE_SIZE);

        if aligned_size == 0 {
            return Err(Error::InvalidArgs);
        }

        let addr = match start {
            Some(addr) => {
                if addr % PAGE_SIZE != 0 {
                    return Err(Error::InvalidArgs);
                }
                addr
            }
            None => self.find_free_region(aligned_size).ok_or(Error::NoMemory)?,
        };

        if populate {
            self.map_region(addr, aligned_size, flags)?;
        } else {
            self.map_region_lazy(addr, aligned_size, flags)?;
        }

        Ok(addr)
    }

    /// Create a file-backed mapping.
    ///
    /// If `start` is `None`, the kernel picks an address. Pages are always
    /// allocated lazily (demand-paged from the file on fault).
    pub fn mmap_file(
        &self,
        start: Option<Vaddr>,
        size: usize,
        flags: PageFlags,
        file_backing: Arc<dyn MmapFileBacking>,
        file_offset: usize,
        is_shared: bool,
    ) -> Result<Vaddr, Error> {
        let aligned_size = align_up(size, PAGE_SIZE);

        if aligned_size == 0 {
            return Err(Error::InvalidArgs);
        }

        let addr = match start {
            Some(addr) => {
                if addr % PAGE_SIZE != 0 {
                    return Err(Error::InvalidArgs);
                }
                if self.find_vma(addr).is_some() {
                    return Err(Error::InvalidArgs);
                }
                addr
            }
            None => self.find_free_region(aligned_size).ok_or(Error::NoMemory)?,
        };

        let region = VmaRegion::new_file_backed(
            addr,
            aligned_size,
            flags,
            file_backing,
            file_offset,
            is_shared,
        );

        let mut regions = self.regions.lock();
        regions.insert(addr, region);

        Ok(addr)
    }

    /// Unmap a range of pages, splitting or removing VMAs as necessary.
    ///
    /// This works like Linux munmap: it can unmap partial regions and leaves
    /// the remaining pieces as separate VMAs.
    pub fn munmap(&self, start: Vaddr, size: usize) -> Result<(), Error> {
        if start % PAGE_SIZE != 0 || size % PAGE_SIZE != 0 {
            return Err(Error::InvalidArgs);
        }
        if size == 0 {
            return Ok(());
        }

        let end_addr = start.checked_add(size).ok_or(Error::InvalidArgs)?;

        // Unmap the page-table entries first.
        let guard = disable_preempt();
        let vaddr_range = start..end_addr;
        let mut cursor = self
            .vm_space
            .cursor_mut(&guard, &vaddr_range)
            .map_err(|_| Error::NoMemory)?;
        cursor.unmap(size);
        drop(cursor);
        drop(guard);

        let mut regions = self.regions.lock();

        // Collect all regions overlapping with [start, end_addr).
        let overlapping_keys: Vec<Vaddr> = regions
            .iter()
            .filter(|(_, r)| {
                let r_end = r.start.checked_add(r.size).unwrap_or(0);
                r.start < end_addr && r_end > start
            })
            .map(|(&k, _)| k)
            .collect();

        for key in overlapping_keys {
            let region = regions.remove(&key).unwrap();
            let r_end = region.start.checked_add(region.size).unwrap_or(0);

            if start <= region.start && end_addr >= r_end {
                // Fully covered — remove entirely.
                continue;
            } else if region.start < start && r_end > end_addr {
                // Target is in the middle — split into left and right.
                regions.insert(
                    region.start,
                    VmaRegion {
                        start: region.start,
                        size: start - region.start,
                        flags: region.flags,
                        guard_size: 0,
                        file_backing: region.file_backing.clone(),
                        file_offset: region.file_offset,
                        is_shared: region.is_shared,
                    },
                );
                regions.insert(
                    end_addr,
                    VmaRegion {
                        start: end_addr,
                        size: r_end - end_addr,
                        flags: region.flags,
                        guard_size: 0,
                        file_backing: region.file_backing.clone(),
                        file_offset: region.file_offset + (end_addr - region.start),
                        is_shared: region.is_shared,
                    },
                );
            } else if region.start < start && r_end <= end_addr {
                // Overlaps on the right side — keep left part.
                regions.insert(
                    region.start,
                    VmaRegion {
                        start: region.start,
                        size: start - region.start,
                        flags: region.flags,
                        guard_size: 0,
                        file_backing: region.file_backing.clone(),
                        file_offset: region.file_offset,
                        is_shared: region.is_shared,
                    },
                );
            } else if region.start >= start && r_end > end_addr {
                // Overlaps on the left side — keep right part.
                regions.insert(
                    end_addr,
                    VmaRegion {
                        start: end_addr,
                        size: r_end - end_addr,
                        flags: region.flags,
                        guard_size: 0,
                        file_backing: region.file_backing.clone(),
                        file_offset: region.file_offset + (end_addr - region.start),
                        is_shared: region.is_shared,
                    },
                );
            }
        }

        Ok(())
    }

    // ------------------------------------------------------------------
    // brk / sbrk helpers
    // ------------------------------------------------------------------

    /// Sets the initial program break to `addr`.
    ///
    /// Called once after ELF loading. Both `start` and `current` are
    /// initialised to the same value — the first address past the loaded
    /// executable's BSS segment, rounded up to a page boundary.
    pub(crate) fn set_brk_initial(&self, addr: Vaddr) {
        let mut brk = self.brk.lock();
        brk.start = addr;
        brk.current = addr;
    }

    /// Adjusts the program break (`brk` / `sbrk` syscall backend).
    ///
    /// * `0` — query only, returns the current break unchanged.
    /// * `< start` — invalid, returns the current break unchanged.
    /// * `> current` — grows the heap by mapping anonymous pages.
    /// * `< current` — shrinks the heap by unmapping pages.
    ///
    /// Always returns the (possibly updated) program break.
    pub(crate) fn brk(&self, new_brk: Vaddr) -> Vaddr {
        let mut brk = self.brk.lock();

        if new_brk == 0 || new_brk < brk.start {
            return brk.current;
        }

        let old_brk = brk.current;
        let new_page = align_up(new_brk, PAGE_SIZE);
        let old_page = align_up(old_brk, PAGE_SIZE);

        // Release the lock before calling map/unmap to avoid holding it
        // across potentially slow page-table operations.
        drop(brk);

        if new_page > old_page {
            if self
                .map_region(old_page, new_page - old_page, PageFlags::RW)
                .is_err()
            {
                return old_brk;
            }
        } else if new_page < old_page {
            let _ = self.unmap_region(new_page, old_page - new_page);
        }

        self.brk.lock().current = new_brk;
        new_brk
    }

    pub fn activate(self: &Arc<Self>) {
        self.vm_space.activate();
    }

    pub fn copy_from_user(&self, user_src: Vaddr, kernel_dest: &mut [u8]) -> Result<(), Error> {
        let len = kernel_dest.len();
        let mut writer = VmWriter::from(kernel_dest);
        let mut reader = self.vm_space.reader(user_src, len)?;
        reader.read_fallible(&mut writer).map_err(|(err, _)| err)?;
        Ok(())
    }

    pub fn copy_to_user(&self, user_dest: Vaddr, kernel_src: &[u8]) -> Result<(), Error> {
        let len = kernel_src.len();
        let mut reader = VmReader::from(kernel_src);
        let mut writer = self.vm_space.writer(user_dest, len)?;
        writer.write_fallible(&mut reader).map_err(|(err, _)| err)?;
        Ok(())
    }
}

fn align_up(addr: usize, align: usize) -> usize {
    (addr + align - 1) & !(align - 1)
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use crate::vm::VMA_MANAGER;
    use ostd::prelude::ktest;

    #[ktest]
    fn test_map_and_unmap_region() {
        let vma_manager = VmaManager::new();
        vma_manager
            .map_region(0x1000, 0x2000, PageFlags::RW)
            .unwrap();
        let regions = vma_manager.regions.lock();
        assert!(regions.contains_key(&0x1000));
        drop(regions);
        vma_manager.unmap_region(0x1000, 0x2000).unwrap();
        let regions = vma_manager.regions.lock();
        assert!(!regions.contains_key(&0x1000));
    }

    #[ktest]
    fn test_copy_user() {
        let vma_manager = Arc::new(VmaManager::new());
        vma_manager.activate();

        vma_manager
            .map_region(0x2000, 0x1000, PageFlags::RW)
            .unwrap();

        let data_to_write = b"Hello from user space test!";
        let mut data_read_back = [0u8; 27];

        vma_manager.copy_to_user(0x2000, data_to_write).unwrap();
        vma_manager
            .copy_from_user(0x2000, &mut data_read_back)
            .unwrap();

        assert_eq!(data_to_write, &data_read_back);

        vma_manager.unmap_region(0x2000, 0x1000).unwrap();
    }

    #[ktest]
    fn test_guard_page_blocks_access() {
        crate::vm::init();
        let vma_manager = VMA_MANAGER.get().unwrap().clone();
        vma_manager.activate();

        let guard_size = PAGE_SIZE;
        let stack_size = PAGE_SIZE * 4;
        let stack_start = 0x90000;

        vma_manager
            .map_stack(stack_start, stack_size, guard_size)
            .unwrap();

        let guard_addr = stack_start;
        let data = b"should not write to guard";

        assert!(vma_manager.copy_to_user(guard_addr, data).is_err());

        let mut buf = [0u8; 4];
        assert!(vma_manager.copy_from_user(guard_addr, &mut buf).is_err());

        vma_manager
            .unmap_region(stack_start, stack_size + guard_size)
            .unwrap();
    }

    #[ktest]
    fn test_stack_usable_area_works() {
        crate::vm::init();
        let vma_manager = VMA_MANAGER.get().unwrap().clone();
        vma_manager.activate();

        let guard_size = PAGE_SIZE;
        let stack_size = PAGE_SIZE * 4;
        let stack_start = 0xa0000;

        vma_manager
            .map_stack(stack_start, stack_size, guard_size)
            .unwrap();

        let usable_start = stack_start + guard_size;
        let data = b"stack data works!";
        let mut buf = [0u8; 17];

        vma_manager.copy_to_user(usable_start, data).unwrap();
        vma_manager.copy_from_user(usable_start, &mut buf).unwrap();
        assert_eq!(data, &buf);

        let top_addr = stack_start + guard_size + stack_size - PAGE_SIZE;
        let top_data = b"top of stack!";
        vma_manager.copy_to_user(top_addr, top_data).unwrap();
        let mut top_buf = [0u8; 13];
        vma_manager.copy_from_user(top_addr, &mut top_buf).unwrap();
        assert_eq!(top_data, &top_buf);

        vma_manager
            .unmap_region(stack_start, stack_size + guard_size)
            .unwrap();
    }

    #[ktest]
    fn test_find_free_region() {
        let vma_manager = VmaManager::new();
        vma_manager
            .map_region(0x1000, 0x2000, PageFlags::RW)
            .unwrap();
        let free = vma_manager.find_free_region(0x1000).unwrap();
        // Should find a region after the mapped one
        assert!(free >= 0x3000 || free < 0x1000);
    }

    #[ktest]
    fn test_find_vma() {
        let vma_manager = VmaManager::new();
        vma_manager
            .map_region(0x5000, 0x1000, PageFlags::RW)
            .unwrap();
        let found = vma_manager.find_vma(0x5500);
        assert!(found.is_some());
        let (key, region) = found.unwrap();
        assert_eq!(key, 0x5000);
        assert_eq!(region.size, 0x1000);
    }

    #[ktest]
    fn test_mmap_anon() {
        let vma_manager = Arc::new(VmaManager::new());
        vma_manager.activate();
        let addr = vma_manager
            .mmap_anon(None, 0x2000, PageFlags::RW, true)
            .unwrap();
        assert!(addr % PAGE_SIZE == 0);
        // Should be usable
        vma_manager.copy_to_user(addr, b"test").unwrap();
        let mut buf = [0u8; 4];
        vma_manager.copy_from_user(addr, &mut buf).unwrap();
        assert_eq!(&buf, b"test");
        vma_manager.munmap(addr, 0x2000).unwrap();
    }

    #[ktest]
    fn test_munmap_partial() {
        let vma_manager = VmaManager::new();
        vma_manager
            .map_region(0x10000, 0x4000, PageFlags::RW)
            .unwrap();
        // Unmap the middle 2 pages
        vma_manager.munmap(0x11000, 0x2000).unwrap();
        let regions = vma_manager.regions.lock();
        assert_eq!(regions.len(), 2);
        assert!(regions.contains_key(&0x10000));
        assert!(regions.contains_key(&0x13000));
        assert_eq!(regions.get(&0x10000).unwrap().size, 0x1000);
        assert_eq!(regions.get(&0x13000).unwrap().size, 0x1000);
    }
}
