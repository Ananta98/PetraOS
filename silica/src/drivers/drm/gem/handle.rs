// SPDX-License-Identifier: GPL-2.0

//! Per-file GEM handle and fake-offset tables.

use alloc::{collections::BTreeMap, sync::Arc};

use super::object::GemObject;

/// Base of the fake-offset mmap range (`0x1000_0000_0000`).
///
/// The DRM mmap address space of a DRM file reserves a fixed region for
/// buffer objects (user space cannot mistake a GEM mapping offset for a
/// regular file offset). Slot numbers are encoded at page granularity above
/// this base.
pub const FAKE_OFFSET_BASE: u64 = 0x1000_0000_0000;

/// The per-file map from GEM handles to buffer objects.
///
/// Handles are"value": small integers unique per DRM file description —
/// user space refers to objects only through them.
#[derive(Debug, Default)]
pub struct GemHandleTable {
    next: u32,
    handles: BTreeMap<u32, Arc<GemObject>>,
}

impl GemHandleTable {
    /// Creates an empty handle table.
    pub fn new() -> Self {
        Self {
            next: 1,
            handles: BTreeMap::new(),
        }
    }

    /// Registers `object` and returns a fresh handle for it.
    pub fn insert(&mut self, object: Arc<GemObject>) -> u32 {
        let handle = self.next;
        self.next += 1;
        self.handles.insert(handle, object);
        handle
    }

    /// Looks up the object of `handle`, if it exists.
    pub fn get(&self, handle: u32) -> Option<Arc<GemObject>> {
        self.handles.get(&handle).cloned()
    }

    /// Reverse lookup: the handle referring to `object`, if user space has
    /// one right now (used by `GETFB`).
    pub fn handle_of(&self, object: &super::object::GemObject) -> Option<u32> {
        self.handles
            .iter()
            .find(|(_, owned)| owned.id == object.id)
            .map(|(handle, _)| *handle)
    }

    /// Removes `handle`. Returns `false` when the handle is unknown, which
    /// user space relies on being able to close twice.
    pub fn remove(&mut self, handle: u32) -> bool {
        self.handles.remove(&handle).is_some()
    }
}

/// The per-file map from fake mmap offsets to buffer objects.
///
/// Offsets are granted instead of computed from handles so that a mapping
/// keeps working even if user space closes and reuses the handle while the
/// mapping is alive; this mirrors the Linux tracking behavior.
#[derive(Debug, Default)]
pub struct GemOffsetTable {
    next: u32,
    entries: BTreeMap<u64, Arc<GemObject>>,
}

impl GemOffsetTable {
    /// Creates an empty offset table.
    pub fn new() -> Self {
        Self {
            next: 0,
            entries: BTreeMap::new(),
        }
    }

    /// Grants a fresh fake offset for `object`.
    pub fn grant(&mut self, object: Arc<GemObject>) -> u64 {
        let slot = self.next;
        self.next += 1;
        let offset = offset_for(slot as u64);
        self.entries.insert(offset, object);
        offset
    }

    /// Looks up the object at a fake `offset`, if one is granted.
    ///
    /// The offset must be above [`FAKE_OFFSET_BASE`] and page aligned; ranges
    /// straddling a page within the same grant return the owning grant.
    pub fn get(&self, offset: u64) -> Option<Arc<GemObject>> {
        if offset <= FAKE_OFFSET_BASE {
            return None;
        }
        // Round down to the grant's page-level layout
        let page = (offset - FAKE_OFFSET_BASE) >> 12;
        self.entries.get(&(FAKE_OFFSET_BASE + (page << 12))).cloned()
    }

    /// Removes the entry granting `offset`. `true` when it existed.
    pub fn remove(&mut self, offset: u64) -> bool {
        self.entries.remove(&offset).is_some()
    }
}

fn offset_for(slot: u64) -> u64 {
    FAKE_OFFSET_BASE + (slot << 12)
}
