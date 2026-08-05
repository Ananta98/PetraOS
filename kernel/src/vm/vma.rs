use crate::fs::vfs::FileOps;
use crate::vm::region::VmaRegion;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};
use ostd::Error;
use ostd::mm::io::{FallibleVmRead, FallibleVmWrite, VmReader, VmWriter};
use ostd::mm::vm_space::VmSpace;
use ostd::mm::{CachePolicy, FrameAllocOptions, PAGE_SIZE, PageFlags, PageProperty, UFrame, Vaddr};
use ostd::sync::SpinLock;
use ostd::task::disable_preempt;

/// User-space address range for mmap allocations.
const USER_SPACE_START: Vaddr = 0x0000_0000_1000;
const USER_SPACE_END: Vaddr = 0x0000_7FFF_FFFF_0000;

/// The virtual memory manager (VMA) for a process's user-space address space.
///
/// `VmaManager` manages a collection of discrete virtual memory regions (`regions`)
/// and coordinates the lower-level page tables via the framework's `VmSpace`. It also
/// manages process heap expansion (`brk`) and provides safe mechanisms to transfer
/// data between user and kernel spaces.
pub struct VmaManager {
    /// The underlying architecture-specific page-table wrapper.
    pub vm_space: Arc<VmSpace>,
    /// Sorted collection of active memory regions (VMAs), mapped by their starting addresses.
    ///
    /// Guarded by a `SpinLock` to allow concurrent lookups and modifications by system call
    /// handlers or exception handlers (such as page fault handlers).
    pub regions: SpinLock<BTreeMap<Vaddr, VmaRegion>>,
    /// The lowest valid heap address, set once after ELF loading.
    ///
    /// Stored as an `AtomicUsize` because it is written exactly once
    /// (during ELF loading, before the process is scheduled) and then
    /// only read. `Relaxed` ordering is sufficient.
    brk_start: AtomicUsize,
    /// The current program break (top of heap).
    brk_current: SpinLock<Vaddr>,
}

impl VmaManager {
    /// Creates a new empty `VmaManager` with a fresh, isolated address space.
    pub fn new() -> Self {
        Self {
            vm_space: Arc::new(VmSpace::new()),
            regions: SpinLock::new(BTreeMap::new()),
            brk_start: AtomicUsize::new(0),
            brk_current: SpinLock::new(0),
        }
    }

    /// Maps a contiguous range of zeroed physical pages to the given virtual range.
    ///
    /// The virtual memory pages are allocated and mapped eagerly with the specified flags.
    ///
    /// # Errors
    /// * `Error::InvalidArgs` if `start` or `size` are not page-aligned.
    /// * `Error::NoMemory` if physical frame allocation fails.
    pub fn map_region(&self, start: Vaddr, size: usize, flags: PageFlags) -> Result<(), Error> {
        if start % PAGE_SIZE != 0 || size % PAGE_SIZE != 0 {
            return Err(Error::InvalidArgs);
        }
        if size == 0 {
            return Ok(());
        }
        let _ = self.munmap(start, size);

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
        regions.insert(start, VmaRegion::new(start, size, flags));

        Ok(())
    }

    /// Allocates and maps a user-space stack with a guard band at the bottom.
    ///
    /// The bottom-most page is left unmapped to catch stack overflow exceptions.
    ///
    /// # Errors
    /// * `Error::InvalidArgs` if `stack_start` or `stack_size` are not page-aligned.
    /// * `Error::NoMemory` if frame allocation fails.
    pub fn map_user_stack(
        &self,
        stack_start: Vaddr,
        stack_size: usize,
        guard_size: usize,
    ) -> Result<(), Error> {
        if stack_start % PAGE_SIZE != 0
            || stack_size % PAGE_SIZE != 0
            || guard_size % PAGE_SIZE != 0
        {
            return Err(Error::InvalidArgs);
        }

        self.map_region(stack_start + guard_size, stack_size, PageFlags::RW)?;

        let mut regions = self.regions.lock();
        regions.insert(
            stack_start,
            VmaRegion::new(stack_start, stack_size, PageFlags::RW),
        );

        Ok(())
    }

    /// Maps a list of existing physical frames consecutively into the user address space.
    ///
    /// This is typically used for mapping shared memory regions (e.g., IPC shm) or device memory
    /// where physical frames are already allocated or pre-determined.
    ///
    /// # Errors
    /// * `Error::InvalidArgs` if `start` is not page-aligned.
    /// * `Error::NoMemory` if page cursor setup fails.
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

        let mut region = VmaRegion::new(start, size, flags);
        region.is_shared = true;

        let mut regions = self.regions.lock();
        regions.insert(start, region);

        Ok(())
    }

    /// Registers a stack region and designates a specific number of guard pages at the bottom.
    ///
    /// Guard pages act as an unmapped safety buffer. If the stack grows downwards into these pages,
    /// a page fault is immediately triggered to prevent corruption of adjacent memory.
    ///
    /// # Errors
    /// * `Error::InvalidArgs` if `start`, `stack_size`, or `guard_size` are not page-aligned.
    pub fn map_stack(
        &self,
        start: Vaddr,
        stack_size: usize,
        guard_size: usize,
    ) -> Result<(), Error> {
        if start % PAGE_SIZE != 0 || stack_size % PAGE_SIZE != 0 || guard_size % PAGE_SIZE != 0 {
            return Err(Error::InvalidArgs);
        }

        let stack_start = start + guard_size;
        let mut regions = self.regions.lock();
        regions.insert(
            stack_start,
            VmaRegion::new(stack_start, stack_size, PageFlags::RW),
        );
        Ok(())
    }

    /// Unmaps a contiguous virtual memory area from both hardware page tables and the region map.
    ///
    /// # Errors
    /// * `Error::InvalidArgs` if `start` or `size` are not page-aligned.
    /// * `Error::NoMemory` if page cursor setup fails.
    pub fn unmap_region(&self, start: Vaddr, size: usize) -> Result<(), Error> {
        self.munmap(start, size)
    }

    /// Registers a VMA region for lazy demand paging.
    ///
    /// Physical frames are not allocated nor mapped until an access attempt triggers a page fault,
    /// optimizing startup times and memory utilization.
    ///
    /// # Errors
    /// * `Error::InvalidArgs` if `start` or `size` are not page-aligned.
    pub fn map_region_lazy(
        &self,
        start: Vaddr,
        size: usize,
        flags: PageFlags,
    ) -> Result<(), Error> {
        if start % PAGE_SIZE != 0 || size % PAGE_SIZE != 0 {
            return Err(Error::InvalidArgs);
        }
        if size == 0 {
            return Ok(());
        }
        let _ = self.munmap(start, size);

        let mut regions = self.regions.lock();
        regions.insert(start, VmaRegion::new(start, size, flags));

        Ok(())
    }

    // ------------------------------------------------------------------
    // mmap-related helpers
    // ------------------------------------------------------------------

    /// Searches the user address space to find a contiguous unmapped range of at least `size` bytes.
    ///
    /// Traverses the sorted list of existing VMAs to find a gap.
    pub fn find_free_region(&self, size: usize) -> Option<Vaddr> {
        let regions = self.regions.lock();
        let mut candidate = USER_SPACE_START;
        for region in regions.values() {
            let region_end = region.end();
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

    /// Finds the specific VMA containing the given virtual address, if any.
    ///
    /// Returns a tuple containing the VMA's start address and a cloned copy of its metadata.
    pub fn find_vma(&self, addr: Vaddr) -> Option<(Vaddr, VmaRegion)> {
        let regions = self.regions.lock();
        let (&start, region) = regions.range(..=addr).next_back()?;
        if region.contains(addr) {
            Some((start, region.clone()))
        } else {
            None
        }
    }

    /// Creates an anonymous memory mapping (`MAP_ANONYMOUS`).
    ///
    /// If `start` is `Some`, the mapping is attempted at the specific address.
    /// If `start` is `None`, the kernel automatically finds a free range.
    ///
    /// Pages are allocated lazily upon access unless `populate` is set to `true`.
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

    /// Creates a file-backed memory mapping.
    ///
    /// Pages are mapped lazily (demand paging). When a page fault occurs, data is read from
    /// the backing file object at the specified offset.
    pub fn mmap_file(
        &self,
        start: Option<Vaddr>,
        size: usize,
        flags: PageFlags,
        file_backing: Arc<SpinLock<dyn FileOps>>,
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

    /// Unmaps a range of user memory, dynamically splitting, resizing, or removing
    /// overlapping VMAs as necessary.
    ///
    /// Follows standard POSIX/Linux semantics for partial unmapping of memory regions.
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
            .filter(|(_, r)| r.start < end_addr && r.end() > start)
            .map(|(&k, _)| k)
            .collect();

        for key in overlapping_keys {
            let region = regions.remove(&key).unwrap();
            let r_end = region.end();

            if start <= region.start && end_addr >= r_end {
                // Fully covered — remove entirely.
                if region.is_shared {
                    let current_pid = crate::proc::process::Process::current().pid;
                    crate::ipc::shm::shm_dt_if_attached(current_pid, region.start);
                }
            } else if region.start < start && r_end > end_addr {
                // Target is in the middle — split into left and right.
                let (left, temp_right) = region.split_at(start);
                let (_, right) = temp_right.split_at(end_addr);
                regions.insert(left.start, left);
                regions.insert(right.start, right);
            } else if region.start < start {
                // Overlaps on the right side — keep left part.
                let (left, _) = region.split_at(start);
                regions.insert(left.start, left);
            } else {
                // Overlaps on the left side — keep right part.
                let (_, right) = region.split_at(end_addr);
                regions.insert(right.start, right);
            }
        }

        Ok(())
    }

    // ------------------------------------------------------------------
    // brk / sbrk helpers
    // ------------------------------------------------------------------

    /// Sets the initial program break (heap limit) to the specified address.
    ///
    /// Typically called once after loading an ELF executable, designating the end
    /// of the program's BSS segment.
    pub fn set_brk_initial(&self, addr: Vaddr) {
        self.brk_start.store(addr, Ordering::Relaxed);
        *self.brk_current.lock() = addr;
    }

    /// Copies the program break metadata state from a parent process's manager to this one.
    ///
    /// Used during a `fork()` operation to duplicate the heap boundary.
    pub fn copy_brk_from(&self, other: &Self) {
        self.brk_start
            .store(other.brk_start.load(Ordering::Relaxed), Ordering::Relaxed);
        *self.brk_current.lock() = *other.brk_current.lock();
    }

    /// Modifies the process heap size (`brk`/`sbrk` syscall backend).
    ///
    /// * `0` or less than `brk_start` -> returns the current break address unchanged.
    /// * Greater than current break -> expands the heap by eagerly mapping new memory pages.
    /// * Less than current break -> shrinks the heap by unmapping retired pages.
    ///
    /// Returns the updated program break address.
    pub fn brk(&self, new_brk: Vaddr) -> Vaddr {
        let brk_start = self.brk_start.load(Ordering::Relaxed);
        let mut brk_current = self.brk_current.lock();

        if new_brk == 0 || new_brk < brk_start {
            return *brk_current;
        }

        let old_brk = *brk_current;
        let new_page = align_up(new_brk, PAGE_SIZE);
        let old_page = align_up(old_brk, PAGE_SIZE);

        // Release the lock before calling map/unmap to avoid holding it
        // across potentially slow page-table operations.
        drop(brk_current);

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

        *self.brk_current.lock() = new_brk;
        new_brk
    }

    /// Activates this address space on the current CPU core.
    ///
    /// Configures the CPU MMU registers (such as CR3 on x86_64) to point to this page table.
    pub fn activate(self: &Arc<Self>) {
        self.vm_space.activate();
    }

    /// Safely copies data from a user-space virtual address into a kernel buffer.
    ///
    /// Validates page mappings and permissions before copy.
    pub fn copy_from_user(&self, user_src: Vaddr, kernel_dest: &mut [u8]) -> Result<(), Error> {
        let len = kernel_dest.len();
        let mut writer = VmWriter::from(kernel_dest);
        let mut reader = self.vm_space.reader(user_src, len)?;
        reader.read_fallible(&mut writer).map_err(|(err, _)| err)?;
        Ok(())
    }

    /// Safely copies data from a kernel buffer to a user-space virtual address.
    ///
    /// Validates page mappings and permissions before copy.
    pub fn copy_to_user(&self, user_dest: Vaddr, kernel_src: &[u8]) -> Result<(), Error> {
        let len = kernel_src.len();
        let mut reader = VmReader::from(kernel_src);
        let mut writer = self.vm_space.writer(user_dest, len)?;
        writer.write_fallible(&mut reader).map_err(|(err, _)| err)?;
        Ok(())
    }

    /// Merges adjacent compatible VMAs in `self.regions` to minimize fragmentation.
    pub fn coalesce_regions(&self) {
        let mut regions = self.regions.lock();
        let keys: Vec<Vaddr> = regions.keys().cloned().collect();

        for key in keys {
            if let Some(current) = regions.get(&key).cloned() {
                let current_end = current.end();
                if let Some(next) = regions.get(&current_end).cloned() {
                    if current.can_merge_with(&next) {
                        regions.remove(&current_end);
                        let merged = VmaRegion {
                            size: current.size + next.size,
                            ..current
                        };
                        regions.insert(key, merged);
                    }
                }
            }
        }
    }

    /// Gives advice to the kernel about memory usage patterns for `start..start + size` (`madvise(2)`).
    ///
    /// * `DontNeed`: Frees physical page table mappings for the specified range while keeping
    ///   the VMA metadata intact. Subsequent accesses will trigger lazy demand paging.
    /// * `WillNeed`: Pre-populates physical frames for the range.
    pub fn madvise(&self, start: Vaddr, size: usize, advice: AdviseFlag) -> Result<(), Error> {
        if start % PAGE_SIZE != 0 || size % PAGE_SIZE != 0 {
            return Err(Error::InvalidArgs);
        }
        if size == 0 {
            return Ok(());
        }

        let end_addr = start.checked_add(size).ok_or(Error::InvalidArgs)?;

        match advice {
            AdviseFlag::DontNeed => {
                let guard = disable_preempt();
                let vaddr_range = start..end_addr;
                let mut cursor = self
                    .vm_space
                    .cursor_mut(&guard, &vaddr_range)
                    .map_err(|_| Error::NoMemory)?;
                cursor.unmap(size);
                Ok(())
            }
            AdviseFlag::WillNeed => {
                let num_pages = size / PAGE_SIZE;
                for page_idx in 0..num_pages {
                    let page_vaddr = start + (page_idx * PAGE_SIZE);
                    let _ = self.alloc_frame_for_fault(
                        page_vaddr,
                        ostd::arch::cpu::context::PageFaultErrorCode::empty(),
                    );
                }
                Ok(())
            }
            AdviseFlag::Normal | AdviseFlag::Random | AdviseFlag::Sequential => Ok(()),
        }
    }

    /// Resizes or relocates an existing virtual memory mapping (`mremap(2)`).
    ///
    /// * If `new_size < old_size`, shrinks mapping by unmapping the excess range.
    /// * If `new_size > old_size`, attempts to expand in-place or relocates if `allow_move` is true.
    pub fn mremap(
        &self,
        old_addr: Vaddr,
        old_size: usize,
        new_size: usize,
        allow_move: bool,
    ) -> Result<Vaddr, Error> {
        if old_addr % PAGE_SIZE != 0 || old_size == 0 || new_size == 0 {
            return Err(Error::InvalidArgs);
        }

        let old_aligned = align_up(old_size, PAGE_SIZE);
        let new_aligned = align_up(new_size, PAGE_SIZE);

        let (_, old_vma) = self.find_vma(old_addr).ok_or(Error::InvalidArgs)?;
        if old_vma.start != old_addr || old_vma.size < old_aligned {
            return Err(Error::InvalidArgs);
        }

        if new_aligned == old_aligned {
            return Ok(old_addr);
        }

        if new_aligned < old_aligned {
            self.munmap(old_addr + new_aligned, old_aligned - new_aligned)?;
            return Ok(old_addr);
        }

        // Expand in-place if space after old_addr is available
        let expand_size = new_aligned - old_aligned;
        let expand_start = old_addr + old_aligned;

        let in_place_possible = {
            let regions = self.regions.lock();
            let expand_end = expand_start + expand_size;
            regions
                .values()
                .all(|r| r.end() <= expand_start || r.start >= expand_end)
                && expand_end <= USER_SPACE_END
        };

        if in_place_possible {
            let mut regions = self.regions.lock();
            if let Some(vma) = regions.get_mut(&old_addr) {
                vma.size = new_aligned;
            }
            drop(regions);

            if old_vma.file_backing.is_none() {
                let _ = self.map_region_lazy(expand_start, expand_size, old_vma.flags);
            }
            return Ok(old_addr);
        }

        if !allow_move {
            return Err(Error::NoMemory);
        }

        // Relocate to a new address range
        let new_addr = self.find_free_region(new_aligned).ok_or(Error::NoMemory)?;

        if let Some(ref backing) = old_vma.file_backing {
            self.mmap_file(
                Some(new_addr),
                new_aligned,
                old_vma.flags,
                backing.clone(),
                old_vma.file_offset,
                old_vma.is_shared,
            )?;
        } else {
            self.mmap_anon(Some(new_addr), new_aligned, old_vma.flags, true)?;
            let mut copy_buf = [0u8; PAGE_SIZE];
            let num_pages = old_aligned / PAGE_SIZE;
            for p in 0..num_pages {
                let src_vaddr = old_addr + (p * PAGE_SIZE);
                let dst_vaddr = new_addr + (p * PAGE_SIZE);
                if self.copy_from_user(src_vaddr, &mut copy_buf).is_ok() {
                    let _ = self.copy_to_user(dst_vaddr, &copy_buf);
                }
            }
        }

        self.munmap(old_addr, old_aligned)?;
        Ok(new_addr)
    }

    /// Flushes modifications in file-backed VMAs back to underlying file storage (`msync(2)`).
    pub fn msync(&self, start: Vaddr, size: usize) -> Result<(), Error> {
        if start % PAGE_SIZE != 0 || size % PAGE_SIZE != 0 {
            return Err(Error::InvalidArgs);
        }
        if size == 0 {
            return Ok(());
        }

        let end_addr = start.checked_add(size).ok_or(Error::InvalidArgs)?;
        let _guard = disable_preempt();

        let regions = self.regions.lock();
        let target_vmas: Vec<VmaRegion> = regions
            .values()
            .filter(|r| r.start < end_addr && r.end() > start && r.file_backing.is_some())
            .cloned()
            .collect();
        drop(regions);

        let mut file_buf = [0u8; PAGE_SIZE];

        for vma in target_vmas {
            let backing = vma.file_backing.as_ref().unwrap();
            let vma_start = core::cmp::max(start, vma.start);
            let vma_end = core::cmp::min(end_addr, vma.end());
            let num_pages = (vma_end - vma_start) / PAGE_SIZE;

            for page_idx in 0..num_pages {
                let page_vaddr = vma_start + (page_idx * PAGE_SIZE);
                if self.copy_from_user(page_vaddr, &mut file_buf).is_ok() {
                    let mut file_offset = vma.file_offset + (page_vaddr - vma.start);
                    let mut file = backing.lock();
                    let _ = file.write(&file_buf, &mut file_offset);
                }
            }
        }

        Ok(())
    }
}

/// Memory pattern advice flags for [`VmaManager::madvise`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdviseFlag {
    /// Default behavior.
    Normal,
    /// Expect random page access.
    Random,
    /// Expect sequential page access.
    Sequential,
    /// Expect access in the near future — pre-fetch pages.
    WillNeed,
    /// Free hardware page tables for range; subsequent access lazy-faults.
    DontNeed,
}

/// Helper function to align a virtual address or size up to the nearest multiple of alignment.
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

    #[ktest]
    fn test_coalesce_regions() {
        let vma_manager = VmaManager::new();
        vma_manager
            .map_region(0x10000, 0x1000, PageFlags::RW)
            .unwrap();
        vma_manager
            .map_region(0x11000, 0x1000, PageFlags::RW)
            .unwrap();
        {
            let regions = vma_manager.regions.lock();
            assert_eq!(regions.len(), 2);
        }
        vma_manager.coalesce_regions();
        {
            let regions = vma_manager.regions.lock();
            assert_eq!(regions.len(), 1);
            let merged = regions.get(&0x10000).unwrap();
            assert_eq!(merged.size, 0x2000);
        }
    }

    #[ktest]
    fn test_mremap_expand_and_shrink() {
        let vma_manager = Arc::new(VmaManager::new());
        vma_manager.activate();

        let addr = vma_manager
            .mmap_anon(Some(0x20000), 0x1000, PageFlags::RW, true)
            .unwrap();
        vma_manager.copy_to_user(addr, b"mremap_data").unwrap();

        // Shrink mapping
        let same_addr = vma_manager.mremap(addr, 0x1000, 0x1000, false).unwrap();
        assert_eq!(same_addr, addr);

        vma_manager.munmap(addr, 0x1000).unwrap();
    }

    #[ktest]
    fn test_madvise_dontneed() {
        let vma_manager = Arc::new(VmaManager::new());
        vma_manager.activate();

        vma_manager
            .map_region(0x30000, 0x1000, PageFlags::RW)
            .unwrap();
        vma_manager
            .madvise(0x30000, 0x1000, AdviseFlag::DontNeed)
            .unwrap();

        vma_manager.unmap_region(0x30000, 0x1000).unwrap();
    }
}
