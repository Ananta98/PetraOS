// SPDX-License-Identifier: GPL-2.0

//! Locks and completions with Linux `spinlock_t`/`mutex_t`/`completion`
//! semantics, backed by OSTD synchronization primitives.
//!
//! [`SpinLock`] follows the Linux `spinlock_t` convention: locking disables
//! local IRQs for the duration of the guard, so the same lock is safe to take
//! from both process and interrupt context. [`RootSpinLock`] is the equivalent
//! of Linux `raw_spinlock_t` for data that is never touched from IRQ context.

use alloc::sync::Arc;
use core::sync::atomic::{AtomicU32, Ordering};
use ostd::sync::{LocalIrqDisabled, WaitQueue};

/// Linux `spinlock_t`: a spinlock whose guard keeps local IRQs disabled.
pub type SpinLock<T> = ostd::sync::SpinLock<T, LocalIrqDisabled>;

/// Linux `raw_spinlock_t`: a plain spinlock for data never touched from IRQ
/// context, where saving IRQ flags on every access would be pure overhead.
pub type RootSpinLock<T> = ostd::sync::SpinLock<T>;

/// Linux `mutex_t`: a blocking mutex held from process context only.
pub type KMutex<T> = ostd::sync::Mutex<T>;

/// Sentinel count of [`KCompletion`] set by [`KCompletion::complete_all`];
/// all subsequent waits succeed instantly without consuming the count.
const DONE_ALL: u32 = u32::MAX;

/// Linux `completion`: a one-shot-ish event that wakes waiters only after
/// [`KCompletion::complete`] has been called, losing no notification even if
/// all waiters start later.
///
/// Exclusive completion (`complete`) increments a count each waiter consumes
/// one of; [`KCompletion::complete_all`] latches the completion so that any
/// number of waiters pass forever, mirroring Linux `complete_all()`.
pub struct KCompletion {
    done: AtomicU32,
    waiters: WaitQueue,
}

impl KCompletion {
    /// Creates a completion in the "not done" state.
    pub const fn new() -> Self {
        Self {
            done: AtomicU32::new(0),
            waiters: WaitQueue::new(),
        }
    }

    /// Resets the completion to the "not done" state.
    pub fn reinit(&self) {
        self.done.store(0, Ordering::Release);
        self.waiters.wake_all();
    }

    /// Signals that one consumption is available and wakes one waiter.
    ///
    /// May be called any number of times; callers waiting once each must
    /// arrange complete/wait pairing themselves, as in Linux.
    pub fn complete(&self) {
        let _ = self
            .done
            .try_update(Ordering::Release, Ordering::Relaxed, |done| {
                if done == DONE_ALL {
                    None
                } else {
                    Some(done + 1)
                }
            });
        self.waiters.wake_one();
    }

    /// Signals completion for all current and future waiters, permanently.
    pub fn complete_all(&self) {
        self.done.store(DONE_ALL, Ordering::Release);
        self.waiters.wake_all();
    }

    /// Sleeps (spinning at most briefly) until one `complete` is consumed.
    pub fn wait(&self) {
        self.waiters
            .wait_until(|| Self::consume(&self.done).then_some(()));
    }

    /// Consumes one `complete` if available without sleeping.
    pub fn try_wait(&self) -> bool {
        Self::consume(&self.done)
    }

    fn consume(done: &AtomicU32) -> bool {
        done.try_update(Ordering::Acquire, Ordering::Relaxed, |done| {
            match done {
                0 => None,
                DONE_ALL => Some(DONE_ALL),
                _ => Some(done - 1),
            }
            .into()
        })
        .is_ok()
    }
}

impl Default for KCompletion {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience constructor for an `Arc`-shared completion, as required when a
/// completion signals across an owner-transfer boundary (e.g. I/O requests).
pub fn shared_completion() -> Arc<KCompletion> {
    Arc::new(KCompletion::new())
}
