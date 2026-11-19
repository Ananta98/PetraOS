// SPDX-License-Identifier: GPL-2.0

//! One-shot and counting completion events.
//!
//! Waiters sleep until [`Completion::complete`] increments an available signal,
//! losing no notification even if the completion happens before any waiter
//! starts. [`Completion::complete_all`] latches the completion permanently so
//! current and future waiters pass without blocking.

use alloc::sync::Arc;
use core::sync::atomic::{AtomicU32, Ordering};
use ostd::sync::WaitQueue;

/// Sentinel count set by [`Completion::complete_all`]; all subsequent waits
/// succeed immediately without consuming the count.
const DONE_ALL: u32 = u32::MAX;

/// Synchronization primitive for signaling event completion across threads or
/// interrupt handlers.
pub struct Completion {
    done: AtomicU32,
    waiters: WaitQueue,
}

impl Completion {
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

    /// Signals one completion token and wakes a single waiting task.
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

    /// Permanently marks the completion as satisfied and wakes all waiters.
    pub fn complete_all(&self) {
        self.done.store(DONE_ALL, Ordering::Release);
        self.waiters.wake_all();
    }

    /// Sleeps until a completion token can be consumed.
    pub fn wait(&self) {
        self.waiters
            .wait_until(|| Self::consume(&self.done).then_some(()));
    }

    /// Consumes one completion token if available without sleeping.
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

impl Default for Completion {
    fn default() -> Self {
        Self::new()
    }
}

/// Allocates a reference-counted completion for sharing across tasks.
pub fn shared_completion() -> Arc<Completion> {
    Arc::new(Completion::new())
}
