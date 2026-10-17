// SPDX-License-Identifier: GPL-2.0

//! Giving memory back to the kernel.

pub mod scanner;

use alloc::{
    collections::BTreeMap,
    sync::{Arc, Weak},
};
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use ostd::sync::SpinLock;
use ostd::sync::WaitQueue;

/// Something the kernel can ask for memory back.
pub trait ReclaimSource: Send + Sync {
    /// Releases up to `budget` pages, returning how many were released.
    fn reclaim(&self, budget: usize) -> usize;
}

/// The sources a pass asks, keyed by a slot so that nothing has to be found again
/// by value.
type Registry = SpinLock<BTreeMap<usize, Weak<dyn ReclaimSource>>>;

static SOURCES: Registry = SpinLock::new(BTreeMap::new());

/// Wait queue for kswapd daemon.
pub static KSWAPD_WAIT_QUEUE: WaitQueue = WaitQueue::new();

/// Flag to signal kswapd to wake up.
pub static KSWAPD_WAKEUP_FLAG: AtomicBool = AtomicBool::new(false);

/// The next free slot in [`SOURCES`].
static NEXT_SLOT: AtomicUsize = AtomicUsize::new(0);

/// The total number of pages released since boot.
static PAGES_RECLAIMED: AtomicUsize = AtomicUsize::new(0);

/// The number of passes run since boot.
static PASSES: AtomicUsize = AtomicUsize::new(0);

/// Registers `source` as something [`reclaim`] asks.
pub fn register<S: ReclaimSource + 'static>(source: &Arc<S>) {
    let slot = NEXT_SLOT.fetch_add(1, Ordering::Relaxed);
    SOURCES
        .lock()
        .insert(slot, Arc::downgrade(source) as Weak<dyn ReclaimSource>);
}

/// Returns the total number of pages released by reclaim since boot.
pub fn pages_reclaimed() -> usize {
    PAGES_RECLAIMED.load(Ordering::Relaxed)
}

/// Returns the number of reclaim passes run since boot.
pub fn passes() -> usize {
    PASSES.load(Ordering::Relaxed)
}

/// Runs one reclaim pass and returns how many pages it released.
pub fn reclaim(budget: usize) -> usize {
    PASSES.fetch_add(1, Ordering::Relaxed);
    if budget == 0 {
        return 0;
    }

    let mut sources = SOURCES.lock();
    sources.retain(|_, source| source.strong_count() > 0);

    let mut remaining = budget;
    let mut freed = 0;
    for source in sources.values() {
        if remaining == 0 {
            break;
        }
        let Some(source) = source.upgrade() else {
            continue;
        };
        let released = source.reclaim(remaining).min(remaining);
        freed += released;
        remaining -= released;
    }

    PAGES_RECLAIMED.fetch_add(freed, Ordering::Relaxed);
    freed
}

/// Runs reclaim passes until `pages` have been released or a pass comes back
/// empty-handed, returning how many pages were released in total.
pub fn reclaim_at_least(pages: usize) -> usize {
    let mut freed = 0;
    while freed < pages {
        let released = reclaim(pages - freed);
        if released == 0 {
            break;
        }
        freed += released;
    }
    freed
}

/// Spawns the background page daemon (`kswapd0`).
pub fn init_kswapd() {
    crate::proc::kthread::kproc_create(
        "kswapd0",
        || loop {
            let _ = reclaim_at_least(32);

            KSWAPD_WAIT_QUEUE.wait_until(|| {
                if KSWAPD_WAKEUP_FLAG.swap(false, Ordering::Acquire) {
                    Some(())
                } else {
                    None
                }
            });
        },
        19, // Lowest priority (nice 19)
    )
    .expect("failed to spawn kswapd0");
}

/// Wakes up the background page daemon (`kswapd0`) to reclaim memory.
pub fn wakeup_kswapd() {
    KSWAPD_WAKEUP_FLAG.store(true, Ordering::Release);
    KSWAPD_WAIT_QUEUE.wake_one();
}
