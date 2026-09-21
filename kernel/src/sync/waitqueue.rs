//! Kernel Wait Queue Primitive
//!
//! Provides event-driven blocking synchronization for kernel subsystems (pipes,
//! PTYs, sockets, and character devices) without busy-yielding.

use alloc::collections::VecDeque;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicBool, Ordering};
use crate::proc::thread::{Thread, ThreadState};
use crate::sync::Mutex;

/// An entry representing a thread waiting on a [`WaitQueue`].
struct WaitEntry {
    thread: Arc<Mutex<Thread>>,
    woken: Arc<AtomicBool>,
}

/// A kernel wait queue for event-driven thread blocking and wakeups.
pub struct WaitQueue {
    waiters: Mutex<VecDeque<WaitEntry>>,
}

impl WaitQueue {
    /// Create a new, empty wait queue.
    pub const fn new() -> Self {
        Self {
            waiters: Mutex::new(VecDeque::new()),
        }
    }

    /// Block the current thread on this wait queue.
    ///
    /// The `unlock` closure is invoked after the thread is enqueued in the wait queue
    /// but before yielding the CPU, preventing the classic lost-wakeup race condition.
    pub fn wait_with<F: FnOnce()>(&self, unlock: F) {
        let current = match crate::sched::current_thread() {
            Some(t) => t,
            None => {
                unlock();
                return;
            }
        };

        let woken = Arc::new(AtomicBool::new(false));
        {
            let mut waiters = self.waiters.lock();
            waiters.push_back(WaitEntry {
                thread: current.clone(),
                woken: woken.clone(),
            });
        }

        // Atomically mark thread state as Sleeping before releasing external lock
        {
            let mut t = current.lock();
            t.state = ThreadState::Sleeping;
        }

        // Release caller's held lock/resource before descheduling
        unlock();

        // If not already woken by a concurrent wake_one/wake_all call, context-switch away
        if !woken.load(Ordering::SeqCst) {
            crate::sched::schedule(false);
        }

        // Restored upon wakeup; ensure state is Running
        let mut t = current.lock();
        t.state = ThreadState::Running;
    }

    /// Convenience wait when no caller lock needs to be released.
    pub fn wait(&self) {
        self.wait_with(|| {});
    }

    /// Wake up the oldest waiting thread in the queue.
    ///
    /// Returns `true` if a thread was found and woken, `false` otherwise.
    pub fn wake_one(&self) -> bool {
        let mut waiters = self.waiters.lock();
        while let Some(entry) = waiters.pop_front() {
            entry.woken.store(true, Ordering::SeqCst);
            Thread::unblock(entry.thread);
            return true;
        }
        false
    }

    /// Wake up all threads waiting in the queue.
    ///
    /// Returns the number of threads woken.
    pub fn wake_all(&self) -> usize {
        let mut waiters = self.waiters.lock();
        let count = waiters.len();
        while let Some(entry) = waiters.pop_front() {
            entry.woken.store(true, Ordering::SeqCst);
            Thread::unblock(entry.thread);
        }
        count
    }

    /// Returns `true` if there are no waiters currently registered.
    pub fn is_empty(&self) -> bool {
        self.waiters.lock().is_empty()
    }

    /// Returns the number of waiting threads currently in the queue.
    pub fn len(&self) -> usize {
        self.waiters.lock().len()
    }
}

impl Default for WaitQueue {
    fn default() -> Self {
        Self::new()
    }
}
