use super::dentry::Dentry;
use crate::sync::spinlock::Spinlock;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;

/// Maximum number of dentries retained in the global VFS dcache LRU table.
pub const DCACHE_CAPACITY: usize = 2048;

struct DCacheEntry {
    dentry: Arc<Dentry>,
    access_count: u64,
}

/// Global VFS Directory Entry Cache (DCache) with LRU eviction.
pub struct DCache {
    entries: BTreeMap<(usize, String), DCacheEntry>,
    clock: u64,
}

impl DCache {
    /// Create an empty DCache instance.
    pub const fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            clock: 0,
        }
    }

    /// Look up a cached child dentry given its parent dentry and component name.
    pub fn lookup(&mut self, parent: &Arc<Dentry>, name: &str) -> Option<Arc<Dentry>> {
        let parent_ptr = Arc::as_ptr(parent) as usize;
        let key = (parent_ptr, String::from(name));
        if let Some(entry) = self.entries.get_mut(&key) {
            self.clock = self.clock.wrapping_add(1);
            entry.access_count = self.clock;
            return Some(entry.dentry.clone());
        }
        None
    }

    /// Insert a parent-child dentry mapping into the DCache.
    pub fn insert(&mut self, parent: &Arc<Dentry>, name: &str, dentry: Arc<Dentry>) {
        let parent_ptr = Arc::as_ptr(parent) as usize;
        let key = (parent_ptr, String::from(name));

        if self.entries.len() >= DCACHE_CAPACITY && !self.entries.contains_key(&key) {
            // Evict least recently used entry (smallest access_count)
            let mut lru_key: Option<(usize, String)> = None;
            let mut min_access = u64::MAX;

            for (k, entry) in self.entries.iter() {
                if entry.access_count < min_access {
                    min_access = entry.access_count;
                    lru_key = Some(k.clone());
                }
            }

            if let Some(evict_k) = lru_key {
                self.entries.remove(&evict_k);
            }
        }

        self.clock = self.clock.wrapping_add(1);
        self.entries.insert(
            key,
            DCacheEntry {
                dentry,
                access_count: self.clock,
            },
        );
    }

    /// Remove a specific dentry entry from the DCache (e.g. on unlink / rmdir).
    pub fn evict(&mut self, parent: &Arc<Dentry>, name: &str) {
        let parent_ptr = Arc::as_ptr(parent) as usize;
        let key = (parent_ptr, String::from(name));
        self.entries.remove(&key);
    }

    /// Purge all cached dentries from the DCache.
    pub fn purge(&mut self) {
        self.entries.clear();
    }
}

/// Global thread-safe VFS dcache table instance.
pub static DCACHE: Spinlock<DCache> = Spinlock::new(DCache::new());

/// Query the global VFS dcache for a cached child dentry.
pub fn dcache_lookup(parent: &Arc<Dentry>, name: &str) -> Option<Arc<Dentry>> {
    DCACHE.lock().lookup(parent, name)
}

/// Cache a resolved child dentry in the global VFS dcache.
pub fn dcache_insert(parent: &Arc<Dentry>, name: &str, dentry: Arc<Dentry>) {
    DCACHE.lock().insert(parent, name, dentry);
}

/// Evict a child dentry from the global VFS dcache.
pub fn dcache_evict(parent: &Arc<Dentry>, name: &str) {
    DCACHE.lock().evict(parent, name);
}

/// Purge all entries from the global VFS dcache.
pub fn dcache_purge() {
    DCACHE.lock().purge();
}
