// SPDX-License-Identifier: GPL-2.0

//! Virtual memory address regions.
//!
//! A [`Vmar`] is the address space of one process or thread. It owns an
//! [`ostd::mm::VmSpace`] — the hardware page table — and the [`Mappings`] that
//! describe what has been placed in it.
//!
//! Every operation here works on page-aligned ranges and reports failures as
//! POSIX [`Errno`](crate::uapi::errno::Errno) values, so that the syscall layer is a
//! thin argument-validation shell around these methods.

pub mod allocator;
pub mod mappings;
pub mod ops;
pub mod page_fault;

use alloc::sync::Arc;
use core::{
    ops::Range,
    sync::atomic::{AtomicUsize, Ordering},
};

use ostd::mm::{MAX_USERSPACE_VADDR, PAGE_SIZE, Vaddr, VmSpace};
use ostd::sync::{PreemptDisabled, RwLock, RwLockReadGuard};

use crate::uapi::errno::Result;

pub use self::allocator::NoRoom;
pub use self::mappings::VmMapping;
pub use self::mappings::{CarveOutcome, Mappings};

/// The lowest address a mapping may occupy.
///
/// This mirrors Linux's `mmap_min_addr` default of 64 KiB (here 1 MiB). Leaving the first
/// page unmapped means that a null-pointer dereference in user space is
/// guaranteed to fault instead of aliasing something.
pub const VMAR_LOWEST_ADDR: Vaddr = 0x0010_0000;

/// The exclusive upper bound on mapping addresses.
pub const VMAR_CAP_ADDR: Vaddr = MAX_USERSPACE_VADDR;

/// The headroom left above all mappings for a process's main stack to grow
/// into, including its guard page and random offset.
pub const VMAR_STACK_RESERVE: usize = 2048 * PAGE_SIZE;

/// How far below a downward-growing mapping a fault may be and still grow it.
pub const STACK_GUARD_GAP: usize = 256 * PAGE_SIZE;

/// The exclusive upper bound used when `MAP_32BIT` is requested, which asks for
/// an address below 2 GiB.
#[cfg(target_arch = "x86_64")]
pub const MAP_32BIT_HIGH_LIMIT: Vaddr = 0x8000_0000;

/// Returns whether `range1` and `range2` overlap.
pub(crate) fn is_intersected(range1: &Range<Vaddr>, range2: &Range<Vaddr>) -> bool {
    range1.start.max(range2.start) < range1.end.min(range2.end)
}

/// Returns the overlap of two ranges, which the caller must know is non-empty.
pub(crate) fn intersect(range1: &Range<Vaddr>, range2: &Range<Vaddr>) -> Range<Vaddr> {
    debug_assert!(is_intersected(range1, range2));
    range1.start.max(range2.start)..range1.end.min(range2.end)
}

/// Returns whether `range` can be mapped at all.
pub(crate) fn is_mappable_range(range: &Range<Vaddr>) -> bool {
    range.start >= VMAR_LOWEST_ADDR && range.end <= VMAR_CAP_ADDR
}

/// Checks that `range` is a non-empty, page-aligned range inside user space.
pub(crate) fn check_page_aligned_range(range: &Range<Vaddr>) -> Result<()> {
    if range.start >= range.end {
        crate::return_errno!(
            EINVAL,
            "the range {:#x}..{:#x} is empty",
            range.start,
            range.end
        );
    }
    if !range.start.is_multiple_of(PAGE_SIZE) || !range.end.is_multiple_of(PAGE_SIZE) {
        crate::return_errno!(
            EINVAL,
            "the range {:#x}..{:#x} is not page-aligned",
            range.start,
            range.end
        );
    }
    Ok(())
}

/// The address space of a process or thread.
pub struct Vmar {
    /// The records and the total mapped size, which `mmap` checks against the
    /// address space limit while it holds the write lock.
    pub(crate) inner: RwLock<VmarInner>,
    /// The hardware page table.
    pub(crate) vm_space: Arc<VmSpace>,
    /// The address space limit in bytes, or `0` for no limit.
    max_addr_space: AtomicUsize,
    /// Base address of process heap (start break).
    start_brk: AtomicUsize,
    /// Current address of process heap break.
    current_brk: AtomicUsize,
    /// Resident Set Size (RSS) tracking resident page count.
    rss_pages: AtomicUsize,
}

/// The state guarded by [`Vmar::inner`].
#[derive(Default)]
pub(crate) struct VmarInner {
    pub(crate) mappings: Mappings,
    /// The total number of mapped bytes, used for the address space limit.
    pub(crate) total_vm: usize,
}

impl Vmar {
    /// Creates a new, empty address space.
    pub fn new() -> Arc<Self> {
        let vmar = Arc::new(Self {
            inner: RwLock::new(VmarInner {
                mappings: Mappings::new(),
                total_vm: 0,
            }),
            vm_space: Arc::new(VmSpace::new()),
            max_addr_space: AtomicUsize::new(0),
            start_brk: AtomicUsize::new(0),
            current_brk: AtomicUsize::new(0),
            rss_pages: AtomicUsize::new(0),
        });
        // Registered weakly with the memory reclaim subsystem
        crate::vm::reclaim::register(&vmar);
        vmar
    }

    /// Returns the hardware page table backing this address space.
    pub fn vm_space(&self) -> &Arc<VmSpace> {
        &self.vm_space
    }

    /// Installs this address space on the current CPU.
    pub fn activate(&self) {
        self.vm_space.activate();
    }

    /// Returns the total number of mapped bytes.
    pub fn total_mapped_size(&self) -> usize {
        self.inner.read().total_vm
    }

    /// Returns the resident set size in pages (populated physical memory).
    pub fn rss_pages(&self) -> usize {
        self.rss_pages.load(Ordering::Relaxed)
    }

    /// Adds pages to the resident set size counter.
    pub(crate) fn add_rss(&self, pages: usize) {
        self.rss_pages.fetch_add(pages, Ordering::Relaxed);
    }

    /// Subtracts pages from the resident set size counter.
    pub(crate) fn sub_rss(&self, pages: usize) {
        self.rss_pages.fetch_sub(pages, Ordering::Relaxed);
    }

    /// Returns the address space limit in bytes, or `None` if there is none.
    pub fn max_addr_space(&self) -> Option<usize> {
        match self.max_addr_space.load(Ordering::Relaxed) {
            0 => None,
            limit => Some(limit),
        }
    }

    /// Sets the address space limit in bytes. `None` removes the limit.
    pub fn set_max_addr_space(&self, limit: Option<usize>) {
        self.max_addr_space
            .store(limit.unwrap_or(0), Ordering::Relaxed);
    }

    /// Returns the start break address of the heap.
    pub fn start_brk(&self) -> usize {
        self.start_brk.load(Ordering::Relaxed)
    }

    /// Sets the start break address of the heap.
    pub fn set_start_brk(&self, val: usize) {
        self.start_brk.store(val, Ordering::Relaxed);
    }

    /// Returns the current break address of the heap.
    pub fn current_brk(&self) -> usize {
        self.current_brk.load(Ordering::Relaxed)
    }

    /// Sets the current break address of the heap.
    pub fn set_current_brk(&self, val: usize) {
        self.current_brk.store(val, Ordering::Relaxed);
    }

    /// Initializes both start break and current break.
    pub fn init_brk(&self, val: usize) {
        self.start_brk.store(val, Ordering::Relaxed);
        self.current_brk.store(val, Ordering::Relaxed);
    }

    /// Returns whether any existing mapping overlaps the given range.
    pub fn has_overlap(&self, range: &Range<Vaddr>) -> bool {
        self.inner.read().mappings.count_overlap(range) != 0
    }

    /// Returns the mappings that intersect `range`, in ascending address order.
    pub fn mappings_in(&self, range: Range<Vaddr>) -> VmarQuery<'_> {
        VmarQuery {
            inner: self.inner.read(),
            range,
        }
    }

    /// Returns whether every page of `range` is mapped.
    pub fn is_fully_mapped(&self, range: &Range<Vaddr>) -> bool {
        self.inner.read().mappings.is_fully_mapped(range)
    }

    /// Returns the mapping covering `address`, growing a downward-growing one
    /// downwards to reach it, for the fault path.
    pub(super) fn grow_for_fault(&self, address: Vaddr) -> Option<VmMapping> {
        let page = address & !(PAGE_SIZE - 1);

        let mut inner = self.inner.write();
        if let Some(mapping) = inner.mappings.get(address) {
            return Some(mapping.dup());
        }
        inner.grow_downwards(self, page)
    }

    /// Releases every mapping and every page table entry.
    pub fn clear(&self) {
        let mut inner = self.inner.write();
        let vm_space = &self.vm_space;

        for mapping in inner.mappings.iter() {
            if mapping.is_shared() {
                if let Some(backing) = mapping.backing() {
                    let _ = backing
                        .flush(mapping.backing_offset()..mapping.backing_offset() + mapping.len());
                }
            }
        }

        let full_range = 0..VMAR_CAP_ADDR;
        let guard = ostd::task::disable_preempt();
        let mut cursor = vm_space
            .cursor_mut(&guard, &full_range)
            .expect("the VM space covers the whole range");
        cursor.unmap(full_range.len());
        cursor.flusher().sync_tlb_flush();

        inner.mappings.clear();
        inner.total_vm = 0;
        self.rss_pages.store(0, Ordering::Relaxed);
    }
}

impl Drop for Vmar {
    fn drop(&mut self) {
        self.clear();
    }
}

/// A borrowed view of the mappings that intersect a range.
pub struct VmarQuery<'a> {
    inner: RwLockReadGuard<'a, VmarInner, PreemptDisabled>,
    range: Range<Vaddr>,
}

impl VmarQuery<'_> {
    /// Returns the intersecting mappings, in ascending address order.
    pub fn iter(&self) -> alloc::vec::IntoIter<&VmMapping> {
        self.inner.mappings.iter_in(&self.range).into_iter()
    }

    /// Returns whether every page of the range is mapped.
    pub fn is_fully_mapped(&self) -> bool {
        self.inner.mappings.is_fully_mapped(&self.range)
    }

    /// Returns how many mappings intersect the range.
    pub fn len(&self) -> usize {
        self.inner.mappings.iter_range(&self.range).count()
    }

    /// Returns whether no mapping intersects the range.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl VmarInner {
    /// Fails with `ENOMEM` if mapping `extra` more bytes would exceed the
    /// address space limit.
    pub(crate) fn check_fits_addr_space(&self, vmar: &Vmar, extra: usize) -> Result<()> {
        self.check_total_fits(vmar, self.total_vm.saturating_add(extra))
    }

    /// Fails with `ENOMEM` if the address space would end up holding more than
    /// the limit allows.
    pub(crate) fn check_total_fits(&self, vmar: &Vmar, total: usize) -> Result<()> {
        let Some(limit) = vmar.max_addr_space() else {
            return Ok(());
        };
        if total > limit {
            crate::return_errno!(
                ENOMEM,
                "{total} bytes of mappings would exceed the address space limit of {limit}"
            );
        }
        Ok(())
    }

    /// Applies `op` to the part of every mapping that intersects `range`, as
    /// described by [`Mappings::carve`], and keeps the mapped-size total right.
    pub(crate) fn carve<F>(&mut self, range: &Range<Vaddr>, op: F) -> CarveOutcome
    where
        F: FnMut(VmMapping, &Range<Vaddr>) -> Option<VmMapping>,
    {
        let outcome = self.mappings.carve(range, op);
        self.total_vm -= outcome.dropped;
        outcome
    }

    /// Removes the pages of every mapping that intersects `range`.
    pub(crate) fn unmap_range(&mut self, vm_space: &VmSpace, range: &Range<Vaddr>) {
        self.carve(range, |piece, _| {
            piece.unmap(vm_space);
            None
        });
    }

    /// Drops the page table entries of every mapping that intersects `range`,
    /// keeping the mappings themselves.
    pub(crate) fn unmap_pages_only(
        &mut self,
        vm_space: &VmSpace,
        range: &Range<Vaddr>,
    ) -> CarveOutcome {
        self.carve(range, |piece, _| {
            piece.unmap(vm_space);
            Some(piece)
        })
    }

    /// Extends the record above `page` downwards so that it covers `page`, and returns it.
    pub(crate) fn grow_downwards(&mut self, vmar: &Vmar, page: Vaddr) -> Option<VmMapping> {
        if page < VMAR_LOWEST_ADDR {
            return None;
        }
        let above = self.mappings.next(page)?;
        let gap = above.start() - page;
        if gap > STACK_GUARD_GAP {
            return None;
        }
        if self.check_fits_addr_space(vmar, gap).is_err() {
            return None;
        }
        let grown = self.mappings.grow_downwards(page)?;
        self.total_vm += gap;
        Some(grown)
    }

    /// Returns a free region of `size` bytes below `high_limit`, searching from
    /// high addresses to low ones.
    pub(crate) fn find_free_region(
        &self,
        size: usize,
        high_limit: Vaddr,
    ) -> Result<Range<Vaddr>, NoRoom> {
        allocator::find_free_region(&self.mappings, size, high_limit)
    }

    /// Returns the high address limit for an ordinary mapping.
    pub(crate) fn default_high_limit(&self) -> Vaddr {
        VMAR_CAP_ADDR - VMAR_STACK_RESERVE
    }
}

#[cfg(ktest)]
mod tests {
    use core::num::NonZeroUsize;
    use ostd::prelude::ktest;

    use super::*;
    use crate::vm::{
        abi::{flags::MmapFlags, perms::VmPerms},
        backing::Backing,
    };

    fn pages(first: usize, count: usize) -> Range<Vaddr> {
        (first * PAGE_SIZE)..((first + count) * PAGE_SIZE)
    }

    #[ktest]
    fn intersection_helpers() {
        assert!(is_intersected(&pages(10, 2), &pages(11, 1)));
        assert!(!is_intersected(&pages(10, 1), &pages(11, 1)));
        assert_eq!(intersect(&pages(10, 2), &pages(11, 1)), pages(11, 1));
        assert_eq!(intersect(&pages(11, 1), &pages(10, 2)), pages(11, 1));
    }

    #[ktest]
    fn mappable_range_respects_the_bounds() {
        assert!(is_mappable_range(&pages(256, 1)));
        assert!(!is_mappable_range(&pages(255, 1)));
        assert!(!is_mappable_range(&pages(0, 1)));
        assert!(is_mappable_range(
            &(VMAR_CAP_ADDR - PAGE_SIZE..VMAR_CAP_ADDR)
        ));
        assert!(!is_mappable_range(
            &(VMAR_LOWEST_ADDR..VMAR_CAP_ADDR + PAGE_SIZE)
        ));
    }

    #[ktest]
    fn free_region_prefers_the_top() {
        let vmar = Vmar::new();
        let inner = vmar.inner.read();
        let high = inner.default_high_limit();

        let region = inner.find_free_region(PAGE_SIZE, high).unwrap();
        assert_eq!(region.end, high);
    }

    #[ktest]
    fn free_region_skips_occupied_space() {
        let vmar = Vmar::new();
        let high = vmar.inner.read().default_high_limit();
        let mut inner = vmar.inner.write();

        let at_high = high - PAGE_SIZE;
        let record = VmMapping::new(
            at_high,
            NonZeroUsize::new(PAGE_SIZE).unwrap(),
            VmPerms::READ.with_ceiling(),
            false,
            false,
            None::<Arc<dyn Backing>>,
            0,
        );
        inner.mappings.insert(record);
        inner.total_vm += PAGE_SIZE;

        let region = inner.find_free_region(PAGE_SIZE, high).unwrap();
        assert_eq!(region.end, at_high);
    }

    #[ktest]
    fn free_region_fails_when_full() {
        let vmar = Vmar::new();
        let inner = vmar.inner.read();
        let too_big = inner.default_high_limit() - VMAR_LOWEST_ADDR + PAGE_SIZE;
        assert_eq!(
            inner.find_free_region(too_big, inner.default_high_limit()),
            Err(NoRoom)
        );
    }

    #[ktest]
    fn vmar_brk_tracking_and_overlap() {
        let vmar = Vmar::new();
        assert_eq!(vmar.start_brk(), 0);
        assert_eq!(vmar.current_brk(), 0);

        vmar.init_brk(0x4000_0000);
        assert_eq!(vmar.start_brk(), 0x4000_0000);
        assert_eq!(vmar.current_brk(), 0x4000_0000);

        vmar.set_current_brk(0x4000_5000);
        assert_eq!(vmar.current_brk(), 0x4000_5000);

        let test_range = 0x4000_0000..0x4000_1000;
        assert!(!vmar.has_overlap(&test_range));

        vmar.mmap_anonymous(
            0x4000_0000,
            PAGE_SIZE,
            VmPerms::READ | VmPerms::WRITE,
            MmapFlags::PRIVATE | MmapFlags::FIXED | MmapFlags::ANONYMOUS,
        )
        .unwrap();

        assert!(vmar.has_overlap(&test_range));
    }
}
