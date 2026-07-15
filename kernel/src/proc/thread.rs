use alloc::string::String;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicI32, AtomicU8, Ordering};

use ostd::sync::WaitQueue;
use ostd::task::{Task, TaskOptions};

use crate::proc::pid_table::Pid;
use crate::scheduler::{SchedClass, TaskData};
use crate::proc::tid_table::{THREAD_TABLE, Tid};

// ---------------------------------------------------------------------------
// ThreadState
// ---------------------------------------------------------------------------

/// Lifecycle states of a kernel thread, mirroring POSIX / Linux thread states.
///
/// The state is stored as an atomic so that it can be inspected or mutated
/// from a different CPU without taking a lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ThreadState {
    /// Created but not yet submitted to the scheduler.
    New = 0,
    /// Eligible to run, waiting in the run queue.
    Ready = 1,
    /// Currently executing on a CPU core.
    Running = 2,
    /// Blocked on I/O or a synchronization primitive.
    Sleeping = 3,
    /// Function returned; resources pending release.
    Finished = 4,
}

impl ThreadState {
    fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::New,
            1 => Self::Ready,
            2 => Self::Running,
            3 => Self::Sleeping,
            _ => Self::Finished,
        }
    }
}

// ---------------------------------------------------------------------------
// KernelThreadInner
// ---------------------------------------------------------------------------

/// Shared mutable state for a kernel thread, placed behind an `Arc` so that
/// both the thread descriptor (`KernelThread`) and the spawned task closure can
/// reach it without a table lookup on the hot path.
struct KernelThreadInner {
    /// Current lifecycle state (atomic — lock-free cross-CPU reads).
    state: AtomicU8,
    /// Exit code written by the thread when it finishes.
    exit_code: AtomicI32,
    /// Sleeping threads that called `join()` on this thread.
    join_queue: WaitQueue,
}

impl KernelThreadInner {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            state: AtomicU8::new(ThreadState::New as u8),
            exit_code: AtomicI32::new(0),
            join_queue: WaitQueue::new(),
        })
    }

    fn state(&self) -> ThreadState {
        ThreadState::from_u8(self.state.load(Ordering::Acquire))
    }

    fn set_state(&self, state: ThreadState) {
        self.state.store(state as u8, Ordering::Release);
    }

    fn exit_code(&self) -> i32 {
        self.exit_code.load(Ordering::Acquire)
    }
}

// ---------------------------------------------------------------------------
// KernelThread
// ---------------------------------------------------------------------------

/// A kernel thread descriptor.
///
/// Each `KernelThread` owns exactly one `ostd::task::Task` and participates in
/// the kernel scheduler.  Multiple threads may belong to the same process (same
/// `pid`), sharing the process's address space and file-descriptor table while
/// each having independent execution contexts.
///
/// # Unix-like model
///
/// This mirrors the Linux model where every schedulable entity is a
/// `task_struct`:
///
/// - Processes are groups of threads that share a PID group leader.
/// - Each thread has its own TID.
/// - The main thread of a process has TID == PID (ensured by the caller in
///   [`Process::spawn_thread`](crate::proc::process::Process::spawn_thread)).
///
/// # Resource management
///
/// `KernelThread` is always heap-allocated inside an `Arc` and registered in
/// [`THREAD_TABLE`].  It is removed from the table once the thread finishes
/// execution and the join queue has been woken.
pub struct KernelThread {
    /// Unique thread identifier.
    pub tid: Tid,

    /// Owning process identifier.
    pub pid: Pid,

    /// Human-readable thread name (defaults to the process name).
    pub name: String,

    /// Shared mutable state — also held by the task closure for zero-lookup
    /// updates from inside the running task.
    pub inner: Arc<KernelThreadInner>,

    /// The underlying OSTD task.
    pub task: Arc<Task>,
}

impl KernelThread {
    // -----------------------------------------------------------------------
    // Constructor
    // -----------------------------------------------------------------------

    /// Build and immediately spawn a kernel thread owned by `pid`.
    ///
    /// `func` is the thread body: it runs to completion inside the OSTD
    /// scheduler and must be `Send + 'static` because the scheduler may
    /// migrate the task across CPUs.
    ///
    /// The returned `Arc<KernelThread>` is registered in [`THREAD_TABLE`] for
    /// the lifetime of the thread.
    ///
    /// # Errors
    ///
    /// Returns `Err(ostd::Error)` if the OSTD task allocation fails (e.g.,
    /// out of kernel stack space).
    pub(crate) fn spawn<F>(pid: Pid, name: &str, func: F) -> Result<Arc<Self>, ostd::Error>
    where
        F: FnOnce() + Send + 'static,
    {
        // Allocate a stable TID before the task starts so that TID is
        // deterministic regardless of scheduling order.
        let tid = Tid::new();
        let inner = KernelThreadInner::new();
        let inner_for_task = inner.clone();

        let task = TaskOptions::new(move || {
            // The scheduler has dispatched us — transition to Running.
            inner_for_task.set_state(ThreadState::Running);

            func();

            // Thread body returned: mark as Finished, wake joiners, remove
            // from the global table.
            inner_for_task.exit_code.store(0, Ordering::Release);
            inner_for_task.set_state(ThreadState::Finished);
            inner_for_task.join_queue.wake_all();
            THREAD_TABLE.unregister(tid);
        })
        .data(TaskData::new(SchedClass::Fair { nice: 0 }, pid, tid))
        .spawn()
        .map_err(|_| ostd::Error::NoMemory)?;

        let thread = Arc::new(KernelThread {
            tid,
            pid,
            name: String::from(name),
            inner,
            task,
        });

        // Register in the global table so callers can look up by TID.
        THREAD_TABLE.register(thread.clone());

        // The thread may already be Running or even Finished by the time we
        // reach here (no ordering guarantee with the scheduler).  Transition
        // from New → Ready only if the task hasn't already started.
        thread
            .inner
            .state
            .compare_exchange(
                ThreadState::New as u8,
                ThreadState::Ready as u8,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .ok();

        Ok(thread)
    }

    // -----------------------------------------------------------------------
    // Lifecycle
    // -----------------------------------------------------------------------

    /// Block the **calling** thread until this thread has finished.
    ///
    /// Returns the thread's exit code once it has transitioned to `Finished`.
    ///
    /// Analogous to `pthread_join()` / Linux `sys_wait4` for threads within
    /// the same process.
    pub fn join(&self) -> i32 {
        self.inner
            .join_queue
            .wait_until(|| (self.inner.state() == ThreadState::Finished).then_some(()));
        self.inner.exit_code()
    }

    /// Mark this thread as sleeping.
    ///
    /// Should only be called by the thread itself when it is about to block on
    /// a synchronization primitive.
    pub fn set_sleeping(&self) {
        self.inner.set_state(ThreadState::Sleeping);
    }

    /// Transition a sleeping thread back to ready.
    ///
    /// Called by the waker after the thread is placed back on the run queue.
    pub fn set_ready(&self) {
        self.inner.set_state(ThreadState::Ready);
    }
}

// ---------------------------------------------------------------------------
// Free function: spawn a detached kernel thread
// ---------------------------------------------------------------------------

/// Spawn a **detached** kernel thread with a given name.
///
/// Detached threads use PID 0 as the owning process sentinel (no user process
/// owns them).  They are used for kernel-internal background work such as idle
/// loops, device pollers, and timer callbacks.
///
/// # Errors
///
/// Propagates the OSTD task allocation error on failure.
pub fn spawn_kernel_thread<F>(name: &str, func: F) -> Result<Arc<KernelThread>, ostd::Error>
where
    F: FnOnce() + Send + 'static,
{
    KernelThread::spawn(Pid::from_raw(0), name, func)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(ktest)]
mod tests {
    use super::*;
    use alloc::sync::Arc;
    use core::sync::atomic::{AtomicBool, Ordering};
    use ostd::prelude::ktest;

    #[ktest]
    fn test_kernel_thread_runs_to_completion() {
        let ran = Arc::new(AtomicBool::new(false));
        let ran_clone = ran.clone();

        let thread = spawn_kernel_thread("test-thread", move || {
            ran_clone.store(true, Ordering::Release);
        })
        .expect("failed to spawn thread");

        // join() blocks until the thread finishes.
        thread.join();
        assert!(ran.load(Ordering::Acquire));
    }

    #[ktest]
    fn test_kernel_thread_join_returns_exit_code() {
        let thread = spawn_kernel_thread("exit-code-thread", || { /* exits normally; code == 0 */
        })
        .expect("failed to spawn thread");

        assert_eq!(thread.join(), 0);
    }

    #[ktest]
    fn test_multiple_threads_run_concurrently() {
        let counter = Arc::new(core::sync::atomic::AtomicU32::new(0));
        let mut handles = alloc::vec::Vec::new();

        for _ in 0..3 {
            let c = counter.clone();
            let handle = spawn_kernel_thread("counter-thread", move || {
                c.fetch_add(1, Ordering::Relaxed);
            })
            .expect("spawn failed");
            handles.push(handle);
        }

        for handle in handles {
            handle.join();
        }

        assert_eq!(counter.load(Ordering::Relaxed), 3);
    }
}
