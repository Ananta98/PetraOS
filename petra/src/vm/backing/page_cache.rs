// SPDX-License-Identifier: GPL-2.0

//! Global shared page cache for clean file-backed pages.
//!
//! Multiple processes mapping the same file (e.g. shared libraries and ELF
//! executables) share the same physical `UFrame`s for clean pages, drastically
//! reducing RAM usage. Evicts pages on memory pressure via [`ReclaimSource`].

use alloc::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
};
use ostd::mm::UFrame;
use spin::{Mutex, Once};

use crate::vm::reclaim::{self, ReclaimSource};

/// Maximum number of clean 4 KiB pages retained in the global page cache (16 MiB).
const MAX_CACHE_PAGES: usize = 4096;

struct PageCacheInner {
    frames: BTreeMap<(u64, usize), UFrame>,
    /// FIFO/clock queue of `(inode_uid, page_offset)` for bounded eviction.
    order: VecDeque<(u64, usize)>,
}

/// Global shared page cache registered as a kernel [`ReclaimSource`].
pub struct GlobalPageCache {
    inner: Mutex<PageCacheInner>,
}

impl GlobalPageCache {
    fn new() -> Self {
        Self {
            inner: Mutex::new(PageCacheInner {
                frames: BTreeMap::new(),
                order: VecDeque::new(),
            }),
        }
    }

    /// Looks up a cached clean page for `(inode_uid, offset)`.
    pub fn get(&self, inode_uid: u64, offset: usize) -> Option<UFrame> {
        self.inner.lock().frames.get(&(inode_uid, offset)).cloned()
    }

    /// Inserts `frame` for `(inode_uid, offset)`, returning an existing frame if
    /// another thread raced to populate the same page.
    pub fn insert_or_get(&self, inode_uid: u64, offset: usize, frame: UFrame) -> UFrame {
        let mut inner = self.inner.lock();
        let key = (inode_uid, offset);
        if let Some(existing) = inner.frames.get(&key) {
            return existing.clone();
        }
        if inner.frames.len() >= MAX_CACHE_PAGES {
            evict_unpinned(&mut inner, 64);
        }
        inner.frames.insert(key, frame.clone());
        inner.order.push_back(key);
        frame
    }

    /// Removes all cached pages belonging to `inode_uid`.
    pub fn invalidate_inode(&self, inode_uid: u64) {
        let mut inner = self.inner.lock();
        inner.frames.retain(|&(uid, _), _| uid != inode_uid);
        inner.order.retain(|&(uid, _)| uid != inode_uid);
    }

    /// Returns the number of cached pages.
    pub fn len(&self) -> usize {
        self.inner.lock().frames.len()
    }
}

/// Evicts up to `budget` pages that are not mapped in any address space (`refcount == 1`).
/// Falls back to dropping oldest entries if the cache exceeds `MAX_CACHE_PAGES`.
fn evict_unpinned(inner: &mut PageCacheInner, budget: usize) -> usize {
    let mut freed = 0;
    let total = inner.order.len();
    for _ in 0..total {
        if freed >= budget {
            break;
        }
        let Some(key) = inner.order.pop_front() else {
            break;
        };
        if let Some(frame) = inner.frames.get(&key) {
            if frame.reference_count() <= 1 {
                inner.frames.remove(&key);
                freed += 1;
            } else {
                // Still mapped by at least one Vmar; keep in rotation.
                inner.order.push_back(key);
            }
        }
    }
    freed
}

impl ReclaimSource for GlobalPageCache {
    fn reclaim(&self, budget: usize) -> usize {
        let mut inner = self.inner.lock();
        evict_unpinned(&mut inner, budget)
    }
}

static GLOBAL_PAGE_CACHE: Once<Arc<GlobalPageCache>> = Once::new();

/// Initializes and registers the global page cache with [`crate::vm::reclaim`].
pub fn init() {
    let cache = GLOBAL_PAGE_CACHE.call_once(|| Arc::new(GlobalPageCache::new()));
    reclaim::register(cache);
}

/// Looks up a cached clean page.
pub fn get(inode_uid: u64, offset: usize) -> Option<UFrame> {
    GLOBAL_PAGE_CACHE.get()?.get(inode_uid, offset)
}

/// Inserts or returns an existing cached clean page.
pub fn insert_or_get(inode_uid: u64, offset: usize, frame: UFrame) -> UFrame {
    if let Some(cache) = GLOBAL_PAGE_CACHE.get() {
        cache.insert_or_get(inode_uid, offset, frame)
    } else {
        frame
    }
}

/// Invalidates all cached pages for `inode_uid`.
pub fn invalidate_inode(inode_uid: u64) {
    if let Some(cache) = GLOBAL_PAGE_CACHE.get() {
        cache.invalidate_inode(inode_uid);
    }
}
