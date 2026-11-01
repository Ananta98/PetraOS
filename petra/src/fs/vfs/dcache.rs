// SPDX-License-Identifier: GPL-2.0

//! Directory Cache (dcache) abstractions for the VFS.

use alloc::string::String;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::fmt::Debug;
use spin::RwLock;

use super::inode::INode;
use super::mount::Mount;

/// FNV-1a 64-bit string hasher for high-speed directory component hashing.
#[inline]
pub fn dentry_hash(name: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &b in name.as_bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// An entry in the Fast Path Dentry Hash Table.
struct HashEntry {
    hash: u64,
    name: String,
    dentry: Arc<DEntry>,
}

/// Native O(1) Open-Addressing Hash Table for VFS directory cache.
///
/// Provides O(1) amortized lookup with linear probing and zero external dependencies,
/// completely avoiding `#![no_std]` rmeta alloc conflicts.
pub struct FastDentryMap {
    entries: Vec<Option<HashEntry>>,
    count: usize,
}

impl FastDentryMap {
    /// Creates a new empty hash table.
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
            count: 0,
        }
    }

    /// Number of elements in the table.
    pub fn len(&self) -> usize {
        self.count
    }

    /// Checks if table is empty.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Looks up a child dentry by name in O(1) expected time.
    pub fn get(&self, name: &str) -> Option<&Arc<DEntry>> {
        if self.entries.is_empty() {
            return None;
        }
        let hash = dentry_hash(name);
        let mask = self.entries.len() - 1;
        let mut idx = (hash as usize) & mask;
        let mut probed = 0;

        while probed < self.entries.len() {
            match &self.entries[idx] {
                Some(entry) => {
                    if entry.hash == hash && entry.name == name {
                        return Some(&entry.dentry);
                    }
                }
                None => return None,
            }
            idx = (idx + 1) & mask;
            probed += 1;
        }
        None
    }

    /// Looks up a child dentry by name and clones the Arc.
    pub fn lookup(&self, name: &str) -> Option<Arc<DEntry>> {
        self.get(name).cloned()
    }

    /// Inserts or updates a child dentry in O(1) expected time.
    pub fn insert(&mut self, child: Arc<DEntry>) {
        if self.entries.is_empty() || (self.count + 1) * 4 >= self.entries.len() * 3 {
            self.grow();
        }
        let hash = dentry_hash(&child.name);
        let mask = self.entries.len() - 1;
        let mut idx = (hash as usize) & mask;

        loop {
            match &mut self.entries[idx] {
                Some(entry) => {
                    if entry.hash == hash && entry.name == child.name {
                        entry.dentry = child;
                        return;
                    }
                }
                None => {
                    self.entries[idx] = Some(HashEntry {
                        hash,
                        name: child.name.clone(),
                        dentry: child,
                    });
                    self.count += 1;
                    return;
                }
            }
            idx = (idx + 1) & mask;
        }
    }

    /// Removes a child dentry by name.
    pub fn remove(&mut self, name: &str) -> Option<Arc<DEntry>> {
        if self.entries.is_empty() {
            return None;
        }
        let hash = dentry_hash(name);
        let mask = self.entries.len() - 1;
        let mut idx = (hash as usize) & mask;
        let mut probed = 0;

        while probed < self.entries.len() {
            if let Some(entry) = &self.entries[idx] {
                if entry.hash == hash && entry.name == name {
                    let removed = entry.dentry.clone();
                    self.entries[idx] = None;
                    self.count -= 1;
                    self.rehash_cluster((idx + 1) & mask);
                    return Some(removed);
                }
            } else {
                return None;
            }
            idx = (idx + 1) & mask;
            probed += 1;
        }
        None
    }

    /// Rehashes the cluster following a deletion to maintain linear probing invariant.
    fn rehash_cluster(&mut self, mut idx: usize) {
        let mask = self.entries.len() - 1;
        while let Some(entry) = self.entries[idx].take() {
            self.count -= 1;
            let target_hash = entry.hash;
            let mut insert_idx = (target_hash as usize) & mask;
            while self.entries[insert_idx].is_some() {
                insert_idx = (insert_idx + 1) & mask;
            }
            self.entries[insert_idx] = Some(entry);
            self.count += 1;
            idx = (idx + 1) & mask;
        }
    }

    /// Clears all entries from the table.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.count = 0;
    }

    /// Retains only entries satisfying predicate `f`.
    pub fn retain<F: FnMut(&str, &Arc<DEntry>) -> bool>(&mut self, mut f: F) {
        let old = core::mem::take(&mut self.entries);
        self.count = 0;
        for item in old.into_iter().flatten() {
            if f(&item.name, &item.dentry) {
                self.insert(item.dentry);
            }
        }
    }

    /// Returns an iterator over all child dentries.
    pub fn values(&self) -> impl Iterator<Item = &Arc<DEntry>> {
        self.entries.iter().filter_map(|e| e.as_ref().map(|x| &x.dentry))
    }

    /// Grows the hash table to the next power of 2.
    fn grow(&mut self) {
        let new_cap = if self.entries.is_empty() {
            16
        } else {
            self.entries.len() * 2
        };
        let mut new_entries = Vec::with_capacity(new_cap);
        for _ in 0..new_cap {
            new_entries.push(None);
        }
        let old_entries = core::mem::replace(&mut self.entries, new_entries);
        self.count = 0;
        let mask = new_cap - 1;

        for item in old_entries.into_iter().flatten() {
            let mut idx = (item.hash as usize) & mask;
            while self.entries[idx].is_some() {
                idx = (idx + 1) & mask;
            }
            self.entries[idx] = Some(item);
            self.count += 1;
        }
    }
}

/// Represents the caching state of an inode for a particular directory entry.
#[derive(Default)]
pub enum EntryState {
    /// Entry is positively cached and contains a link to the inode.
    Present(Arc<INode>),
    /// Entry is negatively cached (we know it doesn't exist).
    NotPresent,
    /// The entry hasn't been looked up yet or was evicted.
    #[default]
    NotCached,
}

/// A Directory Cache Entry (dentry).
///
/// Represents a component of a path in the VFS cache.
pub struct DEntry {
    /// The name of this component.
    pub name: String,

    /// The underlying `INode` this entry points to, along with its cache state.
    pub inode: RwLock<EntryState>,

    /// The parent of this `DEntry`. Weak reference prevents cycles.
    pub parent: Option<Weak<DEntry>>,

    /// Fast-path hash table of all cached children of this entry.
    pub children: RwLock<FastDentryMap>,

    /// A list of filesystems mounted on top of this dentry.
    pub mounts: RwLock<Vec<Arc<Mount>>>,
}

impl DEntry {
    /// Creates a new directory entry.
    pub fn new(name: String, inode: Option<Arc<INode>>, parent: Option<Weak<DEntry>>) -> Self {
        let state = match inode {
            Some(node) => EntryState::Present(node),
            None => EntryState::NotPresent,
        };

        DEntry {
            name,
            inode: RwLock::new(state),
            parent,
            children: RwLock::new(FastDentryMap::new()),
            mounts: RwLock::new(Vec::new()),
        }
    }

    /// Retrieves the currently cached INode, if any.
    pub fn get_inode(&self) -> Option<Arc<INode>> {
        let lock = self.inode.read();
        match &*lock {
            EntryState::Present(inode) => Some(Arc::clone(inode)),
            _ => None,
        }
    }

    /// Checks if this entry is negatively cached (state NotPresent).
    pub fn is_not_present(&self) -> bool {
        matches!(&*self.inode.read(), EntryState::NotPresent)
    }

    /// Updates the cached INode for this entry.
    pub fn set_inode(&self, inode: Option<Arc<INode>>) {
        let mut lock = self.inode.write();
        *lock = match inode {
            Some(node) => EntryState::Present(node),
            None => EntryState::NotPresent,
        };
    }

    /// Looks up a child in the cache via Fast Path O(1) hash lookup.
    pub fn lookup_child(&self, name: &str) -> Option<Arc<DEntry>> {
        self.children.read().lookup(name)
    }

    /// Adds a child to the cache, pruning negative dentries if memory limit is reached.
    pub fn add_child(&self, child: Arc<DEntry>) {
        let mut children = self.children.write();
        if children.len() >= 8192 {
            children.retain(|_, d| !d.is_not_present());
        }
        children.insert(child);
    }

    /// Removes a child from the cache, returning it if it was cached.
    pub fn remove_child(&self, name: &str) -> Option<Arc<DEntry>> {
        self.children.write().remove(name)
    }

    /// Clears all cached child entries (including negative dentries).
    pub fn clear_children(&self) {
        self.children.write().clear();
    }

    /// Reconstructs the absolute path of this dentry by walking up parent links.
    pub fn abs_path(&self) -> String {
        let mut components = Vec::new();
        if !self.name.is_empty() {
            components.push(self.name.clone());
        }

        let mut curr_parent = self.parent.as_ref().and_then(|w| w.upgrade());
        let mut depth = 0;
        while let Some(parent) = curr_parent {
            depth += 1;
            if depth > 256 {
                break;
            }
            if !parent.name.is_empty() {
                components.push(parent.name.clone());
            }
            curr_parent = parent.parent.as_ref().and_then(|w| w.upgrade());
        }

        if components.is_empty() {
            String::from("/")
        } else {
            components.reverse();
            let mut result = String::new();
            for c in components {
                result.push('/');
                result.push_str(&c);
            }
            result
        }
    }
}

impl Debug for DEntry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DEntry").field("name", &self.name).finish()
    }
}
