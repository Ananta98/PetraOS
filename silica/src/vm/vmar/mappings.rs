// SPDX-License-Identifier: GPL-2.0

//! The mappings of a virtual address space.
//!
//! A [`VmMapping`] is the record of one contiguous run of virtual addresses
//! with a single set of properties. A [`Mappings`] holds those records ordered
//! by start address, guaranteed to be pairwise non-overlapping.

use alloc::{collections::BTreeMap, sync::Arc, vec::Vec};
use core::{num::NonZeroUsize, ops::Range};

use ostd::mm::{CachePolicy, PAGE_SIZE, PageFlags, PageProperty, Vaddr, VmSpace, tlb::TlbFlushOp};
use ostd::task::disable_preempt;

use super::intersect;
use crate::vm::{backing::Backing, perms::VmPerms};

/// One contiguous run of virtual addresses sharing a set of properties.
pub struct VmMapping {
    /// The first virtual address of the mapping.
    start: Vaddr,
    /// The length in bytes. Always non-zero.
    len: NonZeroUsize,
    /// The permissions in force, together with the ceiling `mprotect` may not exceed.
    perms: VmPerms,
    /// Whether writes are carried through to the backing object.
    shared: bool,
    /// Whether a fault below this mapping extends it downwards (`MAP_GROWSDOWN`).
    grows_down: bool,
    /// Whether this mapping is locked into RAM (`MAP_LOCKED` / `mlock`).
    locked: bool,
    /// Whether this mapping is excluded from child on `fork(2)` (`MADV_DONTFORK`).
    dont_fork: bool,
    /// Whether this mapping is zeroed in child on `fork(2)` (`MADV_WIPEONFORK`).
    wipe_on_fork: bool,
    /// The object supplying the pages, or `None` for anonymous memory.
    backing: Option<Arc<dyn Backing>>,
    /// The offset within the backing object that `start` maps to.
    backing_offset: usize,
}

impl VmMapping {
    /// Creates a record. `len` must be non-zero and page-aligned.
    pub(super) fn new(
        start: Vaddr,
        len: NonZeroUsize,
        perms: VmPerms,
        shared: bool,
        grows_down: bool,
        backing: Option<Arc<dyn Backing>>,
        backing_offset: usize,
    ) -> Self {
        debug_assert!(start.is_multiple_of(PAGE_SIZE));
        debug_assert!(len.get().is_multiple_of(PAGE_SIZE));
        debug_assert!(backing_offset.is_multiple_of(PAGE_SIZE));
        Self {
            start,
            len,
            perms,
            shared,
            grows_down,
            locked: false,
            dont_fork: false,
            wipe_on_fork: false,
            backing,
            backing_offset,
        }
    }

    /// Returns the virtual address range this record covers.
    pub fn range(&self) -> Range<Vaddr> {
        self.start..self.end()
    }

    /// Returns the first address past the end of this mapping.
    pub fn end(&self) -> Vaddr {
        self.start + self.len.get()
    }

    /// Returns the length in bytes.
    #[expect(clippy::len_without_is_empty, reason = "a mapping is never empty")]
    pub fn len(&self) -> usize {
        self.len.get()
    }

    /// Returns the start address.
    pub fn start(&self) -> Vaddr {
        self.start
    }

    /// Returns the permissions of this mapping.
    pub fn perms(&self) -> VmPerms {
        self.perms
    }

    /// Replaces the permissions in force, keeping the ceiling in place.
    pub(super) fn set_perms(&mut self, perms: VmPerms) {
        self.perms = perms;
    }

    /// Returns whether this mapping is shared with other mappings of its backing object.
    pub fn is_shared(&self) -> bool {
        self.shared
    }

    /// Returns whether this mapping asked to grow downwards.
    pub fn grows_down(&self) -> bool {
        self.grows_down
    }

    /// Returns whether this mapping is locked into RAM.
    pub fn is_locked(&self) -> bool {
        self.locked
    }

    /// Sets whether this mapping is locked into RAM.
    pub fn set_locked(&mut self, locked: bool) {
        self.locked = locked;
    }

    /// Returns whether this mapping is excluded on fork.
    pub fn dont_fork(&self) -> bool {
        self.dont_fork
    }

    /// Sets whether this mapping is excluded on fork.
    pub fn set_dont_fork(&mut self, dont_fork: bool) {
        self.dont_fork = dont_fork;
    }

    /// Returns whether this mapping is wiped (zeroed) on fork.
    pub fn wipe_on_fork(&self) -> bool {
        self.wipe_on_fork
    }

    /// Sets whether this mapping is wiped on fork.
    pub fn set_wipe_on_fork(&mut self, wipe_on_fork: bool) {
        self.wipe_on_fork = wipe_on_fork;
    }

    /// Returns whether a fault below this mapping may extend it.
    pub(super) fn is_growable(&self) -> bool {
        self.grows_down && !self.shared && self.backing.is_none()
    }

    /// Returns the object supplying the pages, if any.
    pub fn backing(&self) -> Option<&Arc<dyn Backing>> {
        self.backing.as_ref()
    }

    /// Returns the offset within the backing object that `start` maps to.
    pub fn backing_offset(&self) -> usize {
        self.backing_offset
    }

    /// Returns whether the offset lies within the backing object.
    pub fn is_backed(&self, offset_in_mapping: usize) -> bool {
        match &self.backing {
            Some(backing) => self.backing_offset + offset_in_mapping < backing.size(),
            None => false,
        }
    }

    /// Returns the offset in the backing object that `start + offset_in_mapping` maps to.
    pub(super) fn backing_offset_of(&self, offset_in_mapping: usize) -> usize {
        self.backing_offset + offset_in_mapping
    }

    /// Returns whether a write to this mapping must copy the page first.
    pub fn is_cow(&self) -> bool {
        !self.shared
    }

    /// Returns whether the pages this mapping installs may be reclaimed.
    pub(crate) fn is_reclaimable(&self) -> bool {
        !self.shared && self.backing.is_none() && !self.locked
    }

    /// Returns the page table flags that this mapping's permissions imply.
    pub fn page_flags(&self) -> PageFlags {
        PageFlags::from(self.perms)
    }

    /// Grows the mapping by `extra` bytes at its high end.
    pub(super) fn enlarge(self, extra: usize) -> Self {
        debug_assert!(extra.is_multiple_of(PAGE_SIZE));
        Self {
            len: NonZeroUsize::new(self.len.get() + extra).expect("a mapping is never empty"),
            ..self
        }
    }

    /// Lowers the start of the record to `page`.
    pub(super) fn extend_downwards(self, page: Vaddr) -> Self {
        debug_assert!(page.is_multiple_of(PAGE_SIZE));
        debug_assert!(page < self.start);
        let extra = self.start - page;
        debug_assert!(extra.is_multiple_of(PAGE_SIZE));
        Self {
            start: page,
            len: NonZeroUsize::new(self.len.get() + extra).expect("a mapping is never empty"),
            ..self
        }
    }

    /// Moves the record to `new_start`, keeping properties and backing.
    pub(super) fn relocate(self, new_start: Vaddr) -> Self {
        debug_assert!(new_start.is_multiple_of(PAGE_SIZE));
        Self {
            start: new_start,
            ..self
        }
    }

    /// Shifts the backing offset by `delta` bytes.
    pub(super) fn rebase(&mut self, delta: usize) {
        debug_assert!(delta.is_multiple_of(PAGE_SIZE));
        self.backing_offset += delta;
    }

    /// Returns an independent record with the same properties.
    pub(crate) fn dup(&self) -> Self {
        Self {
            start: self.start,
            len: self.len,
            perms: self.perms,
            shared: self.shared,
            grows_down: self.grows_down,
            locked: self.locked,
            dont_fork: self.dont_fork,
            wipe_on_fork: self.wipe_on_fork,
            backing: self.backing.clone(),
            backing_offset: self.backing_offset,
        }
    }

    /// Splits the record at `at`, which must be strictly inside it.
    pub(super) fn split(self, at: Vaddr) -> (Self, Self) {
        debug_assert!(self.start < at && at < self.end());
        debug_assert!(at.is_multiple_of(PAGE_SIZE));

        let Self {
            start,
            len,
            perms,
            shared,
            grows_down,
            locked,
            dont_fork,
            wipe_on_fork,
            backing,
            backing_offset,
        } = self;
        let left_len = at - start;

        let left = Self {
            start,
            len: NonZeroUsize::new(left_len).expect("split leaves a non-empty side"),
            perms,
            shared,
            grows_down,
            locked,
            dont_fork,
            wipe_on_fork,
            backing: backing.clone(),
            backing_offset,
        };
        let right = Self {
            start: at,
            len: NonZeroUsize::new(len.get() - left_len).expect("split leaves a non-empty side"),
            perms,
            shared,
            grows_down,
            locked,
            dont_fork,
            wipe_on_fork,
            backing,
            backing_offset: backing_offset + left_len,
        };
        (left, right)
    }

    /// Splits the record so that `sub` is covered by exactly the middle result.
    pub(super) fn split_range(self, sub: &Range<Vaddr>) -> (Option<Self>, Self, Option<Self>) {
        let mine = self.range();
        assert!(
            mine.start <= sub.start && sub.end <= mine.end,
            "the splitting range must lie inside the mapping"
        );

        if sub.start == mine.start && sub.end == mine.end {
            (None, self, None)
        } else if mine.start < sub.start {
            let (left, rest) = self.split(sub.start);
            if sub.end < mine.end {
                let (middle, right) = rest.split(sub.end);
                (Some(left), middle, Some(right))
            } else {
                (Some(left), rest, None)
            }
        } else if sub.end < mine.end {
            let (middle, right) = self.split(sub.end);
            (None, middle, Some(right))
        } else {
            (None, self, None)
        }
    }

    /// Returns whether `left` and `right` are adjacent records that may be merged.
    pub(super) fn can_merge(left: &Self, right: &Self) -> bool {
        left.end() == right.start
            && left.shared == right.shared
            && left.grows_down == right.grows_down
            && left.locked == right.locked
            && left.dont_fork == right.dont_fork
            && left.wipe_on_fork == right.wipe_on_fork
            && left.perms == right.perms
            && match (&left.backing, &right.backing) {
                (None, None) => true,
                (Some(left_backing), Some(right_backing)) => {
                    Arc::ptr_eq(left_backing, right_backing)
                        && left.backing_offset + left.len() == right.backing_offset
                }
                _ => false,
            }
    }

    /// Returns a record covering `left` immediately followed by `right`.
    pub(super) fn merged(left: &Self, right: &Self) -> Self {
        debug_assert!(Self::can_merge(left, right));
        Self {
            start: left.start,
            len: NonZeroUsize::new(left.len() + right.len()).expect("both sides are non-empty"),
            perms: left.perms,
            shared: left.shared,
            grows_down: left.grows_down,
            locked: left.locked,
            dont_fork: left.dont_fork,
            wipe_on_fork: left.wipe_on_fork,
            backing: left.backing.clone(),
            backing_offset: left.backing_offset,
        }
    }

    /// Removes this mapping's page table entries and returns how many pages were released.
    pub(super) fn unmap(&self, vm_space: &VmSpace) -> usize {
        if self.shared {
            if let Some(backing) = &self.backing {
                let _ = backing.flush(self.backing_offset..self.backing_offset + self.len.get());
            }
        }
        let range = self.range();
        let guard = disable_preempt();
        let mut cursor = vm_space
            .cursor_mut(&guard, &range)
            .expect("the mapping lies inside the VM space");
        let num_unmapped = cursor.unmap(range.len());
        num_unmapped
    }

    /// Applies `perms` to the page table entries of this mapping.
    pub(super) fn protect(&self, vm_space: &VmSpace, perms: VmPerms) {
        let mut new_flags = PageFlags::from(perms);
        if !self.shared {
            new_flags -= PageFlags::W;
        }
        let range = self.range();
        let guard = disable_preempt();
        let mut cursor = vm_space
            .cursor_mut(&guard, &range)
            .expect("the mapping lies inside the VM space");

        while cursor.virt_addr() < range.end {
            let remaining = range.end - cursor.virt_addr();
            match cursor.protect_next(remaining, |flags, _| {
                let preserve = *flags & (PageFlags::DIRTY | PageFlags::ACCESSED);
                *flags = new_flags | preserve;
            }) {
                Some(protected) => cursor
                    .flusher()
                    .issue_tlb_flush(TlbFlushOp::for_range(protected)),
                None => break,
            }
        }
        cursor.flusher().dispatch_tlb_flush();
        cursor.flusher().sync_tlb_flush();
    }

    /// Returns the page property that a fault on this mapping installs.
    pub(super) fn fault_property(&self, writable: bool, is_write: bool) -> PageProperty {
        let mut flags = self.page_flags() | PageFlags::ACCESSED;
        if is_write {
            flags |= PageFlags::DIRTY;
        }
        if !writable {
            flags -= PageFlags::W;
        }
        PageProperty::new_user(flags, CachePolicy::Writeback)
    }
}

impl core::fmt::Debug for VmMapping {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("VmMapping")
            .field("range", &self.range())
            .field("perms", &self.perms)
            .field("shared", &self.shared)
            .field("grows_down", &self.grows_down)
            .field("locked", &self.locked)
            .field("dont_fork", &self.dont_fork)
            .field("wipe_on_fork", &self.wipe_on_fork)
            .field("backing_offset", &self.backing_offset)
            .field("backing", &self.backing)
            .finish()
    }
}

/// What [`Mappings::carve`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CarveOutcome {
    pub mapped: usize,
    pub dropped: usize,
}

/// The records of a virtual address space, ordered by start address.
#[derive(Debug, Default)]
pub struct Mappings {
    map: BTreeMap<Vaddr, VmMapping>,
}

impl Mappings {
    pub fn new() -> Self {
        Self {
            map: BTreeMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn get(&self, addr: Vaddr) -> Option<&VmMapping> {
        let mapping = self.map.range(..=addr).next_back().map(|(_, m)| m)?;
        (mapping.end() > addr).then_some(mapping)
    }

    pub fn prev(&self, addr: Vaddr) -> Option<&VmMapping> {
        self.map.range(..addr).next_back().map(|(_, m)| m)
    }

    pub fn next(&self, addr: Vaddr) -> Option<&VmMapping> {
        self.map.range(addr..).next().map(|(_, m)| m)
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &VmMapping> {
        self.map.values()
    }

    /// Zero-allocation iterator over all mappings that intersect `range`.
    pub fn iter_range<'a>(&'a self, range: &Range<Vaddr>) -> impl Iterator<Item = &'a VmMapping> {
        let start_bound = range.start;
        let end_bound = range.end;
        let start_key = self
            .map
            .range(..start_bound)
            .next_back()
            .map(|(k, _)| *k)
            .unwrap_or(0);
        self.map
            .range(start_key..end_bound)
            .map(|(_, m)| m)
            .filter(move |m| m.end() > start_bound && m.start() < end_bound)
    }

    /// Returns every record intersecting `range`, in ascending address order.
    pub fn iter_in(&self, range: &Range<Vaddr>) -> Vec<&VmMapping> {
        self.iter_range(range).collect()
    }

    /// Returns the total length of `range` that is covered by a record.
    pub fn count_overlap(&self, range: &Range<Vaddr>) -> usize {
        self.iter_range(range)
            .map(|m| intersect(range, &m.range()).len())
            .sum()
    }

    /// Returns whether every page of `range` is mapped.
    pub fn is_fully_mapped(&self, range: &Range<Vaddr>) -> bool {
        self.count_overlap(range) == range.len()
    }

    pub(super) fn take_in(&mut self, range: &Range<Vaddr>) -> Vec<VmMapping> {
        let keys: Vec<Vaddr> = self.iter_range(range).map(|m| m.start).collect();
        keys.into_iter()
            .map(|key| {
                self.map
                    .remove(&key)
                    .expect("a key came from this very set")
            })
            .collect()
    }

    pub(super) fn insert(&mut self, mapping: VmMapping) {
        let mut mapping = mapping;

        let prev = self
            .prev(mapping.start)
            .filter(|prev| VmMapping::can_merge(prev, &mapping))
            .map(|prev| prev.start);
        if let Some(key) = prev {
            let left = self
                .map
                .remove(&key)
                .expect("a key came from this very set");
            mapping = VmMapping::merged(&left, &mapping);
        }

        let next = self
            .next(mapping.end())
            .filter(|next| VmMapping::can_merge(&mapping, next))
            .map(|next| next.start);
        if let Some(key) = next {
            let right = self
                .map
                .remove(&key)
                .expect("a key came from this very set");
            mapping = VmMapping::merged(&mapping, &right);
        }

        let replaced = self.map.insert(mapping.start, mapping);
        debug_assert!(replaced.is_none(), "an overlapping record was inserted");
    }

    pub(super) fn remove(&mut self, start: Vaddr) -> Option<VmMapping> {
        self.map.remove(&start)
    }

    pub(super) fn grow_downwards(&mut self, page: Vaddr) -> Option<VmMapping> {
        let above = self.next(page)?;
        if above.start() <= page || !above.is_growable() {
            return None;
        }

        let record = self.remove(above.start())?;
        self.insert(record.extend_downwards(page));

        self.get(page).map(VmMapping::dup)
    }

    pub(super) fn clear(&mut self) {
        self.map.clear();
    }

    pub(super) fn carve<F>(&mut self, range: &Range<Vaddr>, mut op: F) -> CarveOutcome
    where
        F: FnMut(VmMapping, &Range<Vaddr>) -> Option<VmMapping>,
    {
        assert!(range.start.is_multiple_of(PAGE_SIZE) && range.end.is_multiple_of(PAGE_SIZE));
        assert!(range.start < range.end);

        let mut outcome = CarveOutcome::default();
        let mut survivors = Vec::new();

        for piece in self.take_in(range) {
            let sub = intersect(range, &piece.range());
            outcome.mapped += sub.len();

            let (left, middle, right) = piece.split_range(&sub);
            survivors.extend(left);
            survivors.extend(right);
            match op(middle, &sub) {
                Some(kept) => survivors.push(kept),
                None => outcome.dropped += sub.len(),
            }
        }

        for survivor in survivors {
            self.insert(survivor);
        }
        outcome
    }
}
