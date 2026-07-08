use crate::fs::vfs::types::Dentry;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use ostd::sync::SpinLock;

/// Global dentry cache to speed up filename lookup.
pub struct DentryCache {
    /// Maps (parent_inode_num, name) -> Arc<Dentry>
    cache: SpinLock<BTreeMap<(u64, String), Arc<Dentry>>>,
}

impl DentryCache {
    /// Create a new dentry cache instance.
    pub const fn new() -> Self {
        Self {
            cache: SpinLock::new(BTreeMap::new()),
        }
    }

    /// Look up a dentry in the cache by parent inode number and name.
    pub fn lookup(&self, parent_inode_num: u64, name: &str) -> Option<Arc<Dentry>> {
        let cache = self.cache.lock();
        cache.get(&(parent_inode_num, String::from(name))).cloned()
    }

    /// Insert a dentry into the cache.
    pub fn insert(&self, parent_inode_num: u64, name: &str, dentry: Arc<Dentry>) {
        let mut cache = self.cache.lock();
        cache.insert((parent_inode_num, String::from(name)), dentry);
    }

    /// Remove a dentry from the cache.
    pub fn remove(&self, parent_inode_num: u64, name: &str) {
        let mut cache = self.cache.lock();
        cache.remove(&(parent_inode_num, String::from(name)));
    }

    /// Clear all cached dentries.
    pub fn clear(&self) {
        let mut cache = self.cache.lock();
        cache.clear();
    }
}

pub static DENTRY_CACHE: DentryCache = DentryCache::new();
