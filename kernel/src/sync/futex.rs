//! Futex (Fast Userspace Mutex) implementation for PetraOS.
//!
//! Provides core futex primitives (`futex_wait`, `futex_wake`, `futex_requeue`),
//! a global wait-queue manager (`FutexManager`), and high-level synchronization
//! primitives built on top of futex (`FutexMutex`, `FutexCondvar`, `FutexSemaphore`).

use alloc::collections::{BTreeMap, VecDeque};
use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicI32, Ordering};

use crate::arch::{ArchImpl, CpuArch};
use crate::proc::process::ProcessId;
use crate::proc::thread::ThreadState;
use crate::proc::thread_manager::THREAD_MANAGER;
use crate::sched::sched_thread::{SchedThread, ThreadId};
use crate::sched::scheduler::GLOBAL_SCHEDULER;
use crate::sync::spinlock::Spinlock;

// ── Futex Command Constants ──────────────────────────────────────────────────

/// Futex operation commands (matching Linux syscall futex ops).
pub const FUTEX_WAIT: u32 = 0;
pub const FUTEX_WAKE: u32 = 1;
pub const FUTEX_FD: u32 = 2;
pub const FUTEX_REQUEUE: u32 = 3;
pub const FUTEX_CMP_REQUEUE: u32 = 4;
pub const FUTEX_WAKE_OP: u32 = 5;
pub const FUTEX_LOCK_PI: u32 = 6;
pub const FUTEX_UNLOCK_PI: u32 = 7;

/// Flag mask for private (process-local) futexes.
pub const FUTEX_PRIVATE_FLAG: u32 = 128;
pub const FUTEX_CMD_MASK: u32 = !FUTEX_PRIVATE_FLAG;

// ── Futex Error Types ─────────────────────────────────────────────────────────

/// Possible errors returned by futex operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FutexError {
    /// The value at the futex address did not match expected value.
    WouldBlock,
    /// Invalid memory address supplied.
    InvalidAddress,
    /// Unaligned memory address (futex must be 4-byte aligned).
    UnalignedAddress,
    /// Invalid futex operation code.
    InvalidOp,
    /// Timed out while waiting.
    Timeout,
    /// Interrupted by signal or system event.
    Interrupted,
}

// ── Futex Key and Waiter Structures ──────────────────────────────────────────

/// Key identifying a unique futex wait queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FutexKey {
    /// Owning process ID.
    pub pid: ProcessId,
    /// Virtual address of the futex word.
    pub uaddr: usize,
}

impl FutexKey {
    pub const fn new(pid: ProcessId, uaddr: usize) -> Self {
        Self { pid, uaddr }
    }
}

/// A thread waiting on a futex queue.
#[derive(Debug, Clone)]
pub struct FutexWaiter {
    pub tid: ThreadId,
    pub woken: bool,
}

// ── Futex Wait-Queue Manager ─────────────────────────────────────────────────

/// Global futex manager controlling all futex wait queues.
pub struct FutexManager {
    queues: BTreeMap<FutexKey, VecDeque<FutexWaiter>>,
}

impl FutexManager {
    pub const fn new() -> Self {
        Self {
            queues: BTreeMap::new(),
        }
    }

    /// Register a thread as waiting on `key`.
    pub fn add_waiter(&mut self, key: FutexKey, tid: ThreadId) {
        self.queues
            .entry(key)
            .or_insert_with(VecDeque::new)
            .push_back(FutexWaiter { tid, woken: false });
    }

    /// Remove a specific waiter from `key`.
    pub fn remove_waiter(&mut self, key: FutexKey, tid: ThreadId) -> bool {
        if let Some(queue) = self.queues.get_mut(&key) {
            if let Some(pos) = queue.iter().position(|w| w.tid == tid) {
                queue.remove(pos);
                if queue.is_empty() {
                    self.queues.remove(&key);
                }
                return true;
            }
        }
        false
    }

    /// Wake up to `count` threads waiting on `key`.
    ///
    /// Returns the list of woken `ThreadId`s.
    pub fn wake(&mut self, key: FutexKey, count: u32) -> Vec<ThreadId> {
        let mut woken = Vec::new();
        if let Some(queue) = self.queues.get_mut(&key) {
            let to_remove = core::cmp::min(count as usize, queue.len());
            for _ in 0..to_remove {
                if let Some(waiter) = queue.pop_front() {
                    woken.push(waiter.tid);
                }
            }
            if queue.is_empty() {
                self.queues.remove(&key);
            }
        }
        woken
    }

    /// Requeue waiters from `key1` to `key2`.
    ///
    /// Wakes up to `count_wake` threads on `key1` and moves up to `count_requeue`
    /// remaining threads from `key1` queue to `key2` queue.
    ///
    /// Returns (woken_threads, requeued_count).
    pub fn requeue(
        &mut self,
        key1: FutexKey,
        count_wake: u32,
        count_requeue: u32,
        key2: FutexKey,
    ) -> (Vec<ThreadId>, u32) {
        let woken_tids = self.wake(key1, count_wake);
        let mut requeued = 0;

        if let Some(mut queue1) = self.queues.remove(&key1) {
            let to_requeue = core::cmp::min(count_requeue as usize, queue1.len());
            for _ in 0..to_requeue {
                if let Some(waiter) = queue1.pop_front() {
                    self.queues
                        .entry(key2)
                        .or_insert_with(VecDeque::new)
                        .push_back(waiter);
                    requeued += 1;
                }
            }
            if !queue1.is_empty() {
                self.queues.insert(key1, queue1);
            }
        }

        (woken_tids, requeued)
    }

    /// Number of active futex keys in the manager.
    pub fn active_keys(&self) -> usize {
        self.queues.len()
    }
}

/// Global futex manager singleton.
pub static FUTEX_MANAGER: Spinlock<FutexManager> = Spinlock::new(FutexManager::new());

// ── Thread Blocking & Unblocking Helpers ─────────────────────────────────────

/// Blocks the currently executing thread on the calling CPU.
pub fn block_current_thread() {
    let cpu_id = ArchImpl::cpu_id();
    let ints = ArchImpl::disable_interrupts();

    // 1. Update thread state in ThreadManager
    let _tid = {
        let mut tm = THREAD_MANAGER.lock();
        let tid = tm
            .current_thread_id(cpu_id)
            .expect("No running thread on CPU to block");
        if let Some(thread) = tm.threads.get_mut(&tid) {
            thread.state = ThreadState::Blocked;
        }
        tid
    };

    // 2. Remove thread from running in per-CPU scheduler without re-enqueueing
    let next_id = {
        let mut sched = GLOBAL_SCHEDULER.lock();
        if let Some(cpu) = sched.cpu_mut(cpu_id) {
            let _prev = cpu.running.take();
        }
        sched.schedule(cpu_id)
    };

    let target_id = match next_id {
        Some(id) => id,
        None => ThreadId((cpu_id + 100) as u64), // Idle thread
    };

    // 3. Switch context to target task
    crate::proc::thread_manager::switch_to(cpu_id, target_id);

    if ints {
        ArchImpl::enable_interrupts();
    }
}

/// Unblocks a sleeping thread by ID, placing it back in the global scheduler queue.
pub fn unblock_thread(tid: ThreadId) {
    let unblocked = {
        let mut tm = THREAD_MANAGER.lock();
        if let Some(thread) = tm.threads.get_mut(&tid) {
            if thread.state == ThreadState::Blocked {
                thread.state = ThreadState::Ready;
                true
            } else {
                false
            }
        } else {
            false
        }
    };

    if unblocked {
        let sched_thread = SchedThread::new_normal(tid);
        GLOBAL_SCHEDULER.lock().spawn_thread(sched_thread, None);
    }
}

// ── Low-Level Futex Operations ───────────────────────────────────────────────

/// Futex WAIT operation: blocks calling thread if `*uaddr == expected_val`.
pub fn futex_wait(
    uaddr: *const i32,
    expected_val: i32,
    _timeout_ns: Option<u64>,
) -> Result<(), FutexError> {
    if uaddr.is_null() {
        return Err(FutexError::InvalidAddress);
    }
    if (uaddr as usize) % 4 != 0 {
        return Err(FutexError::UnalignedAddress);
    }

    let cpu_id = ArchImpl::cpu_id();
    let (tid, pid) = {
        let tm = THREAD_MANAGER.lock();
        let tid = tm.current_thread_id(cpu_id).ok_or(FutexError::InvalidAddress)?;
        let thread = tm.threads.get(&tid).ok_or(FutexError::InvalidAddress)?;
        (tid, thread.process_id)
    };

    let key = FutexKey::new(pid, uaddr as usize);

    // Atomically verify value and enqueue waiter
    {
        let mut fm = FUTEX_MANAGER.lock();
        let current_val = unsafe { core::ptr::read_volatile(uaddr) };
        if current_val != expected_val {
            return Err(FutexError::WouldBlock);
        }
        fm.add_waiter(key, tid);
    }

    // Block calling thread until woken
    block_current_thread();

    Ok(())
}

/// Futex WAKE operation: wakes up to `count` threads waiting on `uaddr`.
pub fn futex_wake(uaddr: *const i32, count: u32) -> Result<u32, FutexError> {
    if uaddr.is_null() {
        return Err(FutexError::InvalidAddress);
    }
    if (uaddr as usize) % 4 != 0 {
        return Err(FutexError::UnalignedAddress);
    }

    let cpu_id = ArchImpl::cpu_id();
    let pid = {
        let tm = THREAD_MANAGER.lock();
        let tid = tm.current_thread_id(cpu_id).ok_or(FutexError::InvalidAddress)?;
        let thread = tm.threads.get(&tid).ok_or(FutexError::InvalidAddress)?;
        thread.process_id
    };

    let key = FutexKey::new(pid, uaddr as usize);
    let woken_tids = {
        let mut fm = FUTEX_MANAGER.lock();
        fm.wake(key, count)
    };

    let count_woken = woken_tids.len() as u32;
    for tid in woken_tids {
        unblock_thread(tid);
    }

    Ok(count_woken)
}

/// Futex REQUEUE / CMP_REQUEUE operation.
pub fn futex_requeue(
    uaddr1: *const i32,
    count_wake: u32,
    count_requeue: u32,
    uaddr2: *const i32,
    expected_val3: Option<i32>,
) -> Result<u32, FutexError> {
    if uaddr1.is_null() || uaddr2.is_null() {
        return Err(FutexError::InvalidAddress);
    }
    if (uaddr1 as usize) % 4 != 0 || (uaddr2 as usize) % 4 != 0 {
        return Err(FutexError::UnalignedAddress);
    }

    let cpu_id = ArchImpl::cpu_id();
    let pid = {
        let tm = THREAD_MANAGER.lock();
        let tid = tm.current_thread_id(cpu_id).ok_or(FutexError::InvalidAddress)?;
        let thread = tm.threads.get(&tid).ok_or(FutexError::InvalidAddress)?;
        thread.process_id
    };

    let key1 = FutexKey::new(pid, uaddr1 as usize);
    let key2 = FutexKey::new(pid, uaddr2 as usize);

    let (woken_tids, _requeued) = {
        let mut fm = FUTEX_MANAGER.lock();

        if let Some(cmp_val) = expected_val3 {
            let actual_val = unsafe { core::ptr::read_volatile(uaddr1) };
            if actual_val != cmp_val {
                return Err(FutexError::WouldBlock);
            }
        }

        fm.requeue(key1, count_wake, count_requeue, key2)
    };

    let count_woken = woken_tids.len() as u32;
    for tid in woken_tids {
        unblock_thread(tid);
    }

    Ok(count_woken)
}

// ── High-Level Futex Synchronizers ──────────────────────────────────────────

/// Fast Userspace Mutex (`FutexMutex`).
///
/// State transitions:
/// - 0: Unlocked
/// - 1: Locked (no waiters)
/// - 2: Locked (has waiters)
pub struct FutexMutex<T> {
    state: AtomicI32,
    data: UnsafeCell<T>,
}

unsafe impl<T: Send> Send for FutexMutex<T> {}
unsafe impl<T: Send> Sync for FutexMutex<T> {}

pub struct FutexMutexGuard<'a, T> {
    mutex: &'a FutexMutex<T>,
}

impl<T> FutexMutex<T> {
    pub const fn new(data: T) -> Self {
        Self {
            state: AtomicI32::new(0),
            data: UnsafeCell::new(data),
        }
    }

    pub fn lock(&self) -> FutexMutexGuard<'_, T> {
        if self
            .state
            .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            self.lock_slow();
        }
        FutexMutexGuard { mutex: self }
    }

    fn lock_slow(&self) {
        let mut val = self.state.load(Ordering::Relaxed);
        loop {
            if val == 0 {
                match self
                    .state
                    .compare_exchange_weak(0, 1, Ordering::Acquire, Ordering::Relaxed)
                {
                    Ok(_) => return,
                    Err(actual) => {
                        val = actual;
                        continue;
                    }
                }
            }

            if val != 2 {
                if let Err(actual) =
                    self.state
                        .compare_exchange_weak(val, 2, Ordering::Relaxed, Ordering::Relaxed)
                {
                    val = actual;
                    continue;
                }
            }

            let _ = futex_wait(self.state.as_ptr(), 2, None);
            val = self.state.load(Ordering::Relaxed);
        }
    }

    pub fn unlock(&self) {
        if self.state.swap(0, Ordering::Release) == 2 {
            let _ = futex_wake(self.state.as_ptr(), 1);
        }
    }
}

impl<T> Deref for FutexMutexGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.mutex.data.get() }
    }
}

impl<T> DerefMut for FutexMutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.mutex.data.get() }
    }
}

impl<T> Drop for FutexMutexGuard<'_, T> {
    fn drop(&mut self) {
        self.mutex.unlock();
    }
}

/// Futex-backed Condition Variable (`FutexCondvar`).
pub struct FutexCondvar {
    sequence: AtomicI32,
}

impl FutexCondvar {
    pub const fn new() -> Self {
        Self {
            sequence: AtomicI32::new(0),
        }
    }

    pub fn wait<'a, T>(&self, guard: FutexMutexGuard<'a, T>) -> FutexMutexGuard<'a, T> {
        let seq = self.sequence.load(Ordering::Relaxed);
        let mutex = guard.mutex;
        drop(guard);

        let _ = futex_wait(self.sequence.as_ptr(), seq, None);

        mutex.lock()
    }

    pub fn notify_one(&self) {
        self.sequence.fetch_add(1, Ordering::Relaxed);
        let _ = futex_wake(self.sequence.as_ptr(), 1);
    }

    pub fn notify_all(&self) {
        self.sequence.fetch_add(1, Ordering::Relaxed);
        let _ = futex_wake(self.sequence.as_ptr(), u32::MAX);
    }
}

/// Futex-backed Counting Semaphore (`FutexSemaphore`).
pub struct FutexSemaphore {
    count: AtomicI32,
}

impl FutexSemaphore {
    pub const fn new(initial_count: i32) -> Self {
        Self {
            count: AtomicI32::new(initial_count),
        }
    }

    pub fn wait(&self) {
        loop {
            let current = self.count.load(Ordering::Relaxed);
            if current > 0 {
                if self
                    .count
                    .compare_exchange_weak(current, current - 1, Ordering::Acquire, Ordering::Relaxed)
                    .is_ok()
                {
                    return;
                }
            } else {
                let _ = futex_wait(self.count.as_ptr(), current, None);
            }
        }
    }

    pub fn post(&self) {
        self.count.fetch_add(1, Ordering::Release);
        let _ = futex_wake(self.count.as_ptr(), 1);
    }
}

/// Self-test function verifying futex primitives and synchronizers.
pub fn run_self_tests() {
    log::info!("Running Futex self-tests...");

    // Test 1: futex_wait on value mismatch returns WouldBlock
    let word: i32 = 42;
    let res = futex_wait(&word as *const i32, 100, None);
    assert_eq!(res, Err(FutexError::WouldBlock));

    // Test 2: futex_wake on address with no waiters returns 0
    let woken = futex_wake(&word as *const i32, 10).expect("futex_wake failed");
    assert_eq!(woken, 0);

    // Test 3: FutexMutex lock and unlock
    let mutex = FutexMutex::new(100);
    {
        let mut guard = mutex.lock();
        *guard += 50;
        assert_eq!(*guard, 150);
    }

    // Test 4: FutexSemaphore post and wait
    let sem = FutexSemaphore::new(1);
    sem.wait();
    sem.post();

    log::info!("Futex self-tests passed successfully!");
}
