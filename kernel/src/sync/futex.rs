//! Fast Userspace Mutex (Futex) Subsystem
//!
//! Provides kernel-level wait queues and synchronization mechanisms for userspace
//! synchronization primitives (mutexes, condition variables, semaphores, barriers).
//!
//! In userspace, fast-path lock acquisitions execute atomically without entering
//! the kernel. When contention occurs, threads invoke `sys_futex` to block or wake
//! waiting execution contexts via the [`FutexManager`].

use crate::proc::thread::{Thread, ThreadId, ThreadState};
use crate::sync::Mutex;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::sync::Arc;

// Standard Linux Futex Operation Commands
pub const FUTEX_WAIT: u32 = 0;
pub const FUTEX_WAKE: u32 = 1;
pub const FUTEX_FD: u32 = 2;
pub const FUTEX_REQUEUE: u32 = 3;
pub const FUTEX_CMP_REQUEUE: u32 = 4;
pub const FUTEX_WAKE_OP: u32 = 5;
pub const FUTEX_LOCK_PI: u32 = 6;
pub const FUTEX_UNLOCK_PI: u32 = 7;
pub const FUTEX_TRYLOCK_PI: u32 = 8;
pub const FUTEX_WAIT_BITSET: u32 = 9;
pub const FUTEX_WAKE_BITSET: u32 = 10;
pub const FUTEX_WAIT_REQUEUE_PI: u32 = 11;
pub const FUTEX_CMP_REQUEUE_PI: u32 = 12;

// Futex Modifiers and Flags
pub const FUTEX_PRIVATE_FLAG: u32 = 128;
pub const FUTEX_CLOCK_REALTIME: u32 = 256;
pub const FUTEX_CMD_MASK: u32 = !(FUTEX_PRIVATE_FLAG | FUTEX_CLOCK_REALTIME);

// Futex Bitset Wildcard Mask
pub const FUTEX_BITSET_MATCH_ANY: u32 = 0xFFFF_FFFF;

/// Represents an address key for identifying a unique futex word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FutexKey {
    /// Process-private futex (isolated to a single process/address space).
    Private { pid: u64, vaddr: u64 },
    /// Shared memory futex across multiple processes (keyed by physical frame address).
    Shared { paddr: u64 },
}

/// Errors returned by futex operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FutexError {
    WouldBlock,
    TimedOut,
    InvalidArgument,
    Fault,
    Interrupted,
    NotSupported,
}

/// Represents a waiting thread in a futex wait queue.
pub struct FutexWaiter {
    /// Reference to the blocked thread.
    pub thread: Arc<Mutex<Thread>>,
    /// Bitset mask for selective wakeups (used by `FUTEX_WAIT_BITSET`).
    pub bitset: u32,
    /// Absolute monotonic deadline in nanoseconds, if a timeout was specified.
    pub deadline_ns: Option<u64>,
    /// Indicates whether the thread was explicitly woken by a wake/requeue operation.
    pub woken: bool,
}

/// Global manager for kernel futex wait queues.
pub struct FutexManager {
    queues: BTreeMap<FutexKey, VecDeque<FutexWaiter>>,
}

/// Global singleton instance of the Futex Manager.
pub static FUTEX_MANAGER: Mutex<FutexManager> = Mutex::new(FutexManager::new());

impl FutexManager {
    /// Creates a new, empty `FutexManager`.
    pub const fn new() -> Self {
        Self {
            queues: BTreeMap::new(),
        }
    }

    /// Enqueues a thread into the wait queue for `key` if `*uaddr == expected_val`.
    ///
    /// # Safety
    /// `uaddr` must point to valid user-space memory accessible in the current address space.
    pub unsafe fn wait_prepare(
        &mut self,
        key: FutexKey,
        thread: Arc<Mutex<Thread>>,
        uaddr: *const u32,
        expected_val: u32,
        bitset: u32,
        deadline_ns: Option<u64>,
    ) -> Result<(), FutexError> {
        if bitset == 0 {
            return Err(FutexError::InvalidArgument);
        }

        // SAFETY: The caller guarantees `uaddr` is valid and mapped. We perform volatile read.
        let current_val = unsafe { core::ptr::read_volatile(uaddr) };
        if current_val != expected_val {
            return Err(FutexError::WouldBlock);
        }

        let waiter = FutexWaiter {
            thread,
            bitset,
            deadline_ns,
            woken: false,
        };

        self.queues.entry(key).or_default().push_back(waiter);
        Ok(())
    }

    /// Wakes up to `max_count` threads waiting on `key` matching `bitset`.
    ///
    /// Returns the number of threads actually woken and unblocked.
    pub fn wake(&mut self, key: FutexKey, max_count: usize, bitset: u32) -> usize {
        if bitset == 0 || max_count == 0 {
            return 0;
        }

        let mut woken_count = 0;
        let mut remove_key = false;

        if let Some(queue) = self.queues.get_mut(&key) {
            let mut i = 0;
            while i < queue.len() && woken_count < max_count {
                if (queue[i].bitset & bitset) != 0 {
                    let mut waiter = match queue.remove(i) {
                        Some(w) => w,
                        None => break,
                    };
                    waiter.woken = true;
                    Thread::unblock(waiter.thread);
                    woken_count += 1;
                } else {
                    i += 1;
                }
            }
            if queue.is_empty() {
                remove_key = true;
            }
        }

        if remove_key {
            self.queues.remove(&key);
        }

        woken_count
    }

    /// Wakes up to `wake_count` threads on `key1`, and requeues up to `requeue_count`
    /// remaining threads from `key1` to `key2`.
    ///
    /// Returns a tuple of `(woken_count, requeued_count)`.
    pub fn requeue(
        &mut self,
        key1: FutexKey,
        key2: FutexKey,
        wake_count: usize,
        requeue_count: usize,
        bitset: u32,
    ) -> (usize, usize) {
        if bitset == 0 {
            return (0, 0);
        }

        let mut woken_count = 0;
        let mut requeued_count = 0;

        let mut queue1 = match self.queues.remove(&key1) {
            Some(q) => q,
            None => return (0, 0),
        };

        // Step 1: Wake up to `wake_count` waiters
        let mut remaining = VecDeque::new();
        while let Some(mut waiter) = queue1.pop_front() {
            if woken_count < wake_count && (waiter.bitset & bitset) != 0 {
                waiter.woken = true;
                Thread::unblock(waiter.thread);
                woken_count += 1;
            } else {
                remaining.push_back(waiter);
            }
        }

        // Step 2: Requeue up to `requeue_count` remaining waiters to key2
        if key1 != key2 {
            let queue2 = self.queues.entry(key2).or_default();
            while let Some(waiter) = remaining.pop_front() {
                if requeued_count < requeue_count {
                    queue2.push_back(waiter);
                    requeued_count += 1;
                } else {
                    // Put back to queue1 if limit reached
                    remaining.push_front(waiter);
                    break;
                }
            }
        }

        // Put back any non-requeued waiters on key1
        if !remaining.is_empty() {
            self.queues.insert(key1, remaining);
        }

        (woken_count, requeued_count)
    }

    /// Removes a specific thread from a wait queue by its ThreadId (e.g. on signal or timeout).
    ///
    /// Returns `true` if the waiter was found and removed, `false` if it was already woken.
    pub fn remove_waiter(&mut self, key: &FutexKey, tid: ThreadId) -> bool {
        let mut removed = false;
        let mut remove_key = false;

        if let Some(queue) = self.queues.get_mut(key) {
            if let Some(pos) = queue.iter().position(|w| w.thread.lock().tid == tid) {
                queue.remove(pos);
                removed = true;
            }
            if queue.is_empty() {
                remove_key = true;
            }
        }

        if remove_key {
            self.queues.remove(key);
        }

        removed
    }

    /// Checks all wait queues for expired deadlines and wakes timed-out threads.
    pub fn check_timeouts(&mut self, current_ns: u64) {
        let mut keys_to_clean = alloc::vec::Vec::new();

        for (key, queue) in self.queues.iter_mut() {
            let mut i = 0;
            while i < queue.len() {
                if let Some(deadline) = queue[i].deadline_ns {
                    if current_ns >= deadline {
                        let waiter = match queue.remove(i) {
                            Some(w) => w,
                            None => break,
                        };
                        Thread::unblock(waiter.thread);
                        continue;
                    }
                }
                i += 1;
            }
            if queue.is_empty() {
                keys_to_clean.push(*key);
            }
        }

        for key in keys_to_clean {
            self.queues.remove(&key);
        }
    }
}
