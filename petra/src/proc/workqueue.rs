// SPDX-License-Identifier: GPL-2.0

//! Deferred kernel workqueue executed in dedicated worker threads.
//!
//! Work items run in process context, so they may sleep, allocate, or take
//! blocking locks — unlike interrupt handlers. Submission uses an
//! IRQ-disabling spinlock so [`schedule_work`] and [`schedule_work_fn`] are
//! safe to call from interrupt context.

use alloc::{boxed::Box, collections::VecDeque, sync::Arc};
use core::time::Duration;
use ostd::sync::{LocalIrqDisabled, SpinLock, WaitQueue};
use spin::Once;

use crate::proc::kthread::{kproc_create, kthread_add};
use crate::time::timer::{KernelTimer, arm};
use crate::uapi::errno::Result;

type WorkFn = Box<dyn FnOnce() + Send>;

struct Inner {
    pending: SpinLock<VecDeque<WorkFn>, LocalIrqDisabled>,
    queued: WaitQueue,
}

static WORKQ: Once<Arc<Inner>> = Once::new();

/// Number of worker threads serving the global workqueue.
const WORKERS: usize = 4;

/// Returns whether the global workqueue has been initialized.
pub fn is_initialized() -> bool {
    WORKQ.get().is_some()
}

/// Schedules a boxed work item to run in workqueue context.
///
/// # Panics
///
/// Panics if called before [`init`] so ordering bugs surface immediately.
pub fn schedule_work(item: WorkFn) {
    let inner = WORKQ
        .get()
        .cloned()
        .expect("workqueue: schedule_work before init");
    inner.pending.lock().push_back(item);
    inner.queued.wake_one();
}

/// Schedules a closure to run in workqueue context.
pub fn schedule_work_fn<F>(f: F)
where
    F: FnOnce() + Send + 'static,
{
    schedule_work(Box::new(f));
}

/// Schedules a closure to run in workqueue context after `after` elapses.
pub fn schedule_delayed_work_fn<F>(f: F, after: Duration) -> KernelTimer
where
    F: FnOnce() + Send + 'static,
{
    let slot: Arc<SpinLock<Option<WorkFn>, LocalIrqDisabled>> =
        Arc::new(SpinLock::new(Some(Box::new(f))));
    // The timer callback runs in IRQ context and only enqueues the item; the
    // payload executes in worker thread context.
    arm(
        after,
        Some(Box::new(move || {
            if let Some(item) = slot.lock().take() {
                schedule_work(item);
            }
        })),
    )
}

/// Returns the number of work items currently queued.
pub fn pending_work_items() -> usize {
    WORKQ.get().map(|q| q.pending.lock().len()).unwrap_or(0)
}

fn worker_entry() {
    let Some(inner) = WORKQ.get().cloned() else {
        return;
    };
    loop {
        let item = inner.queued.wait_until(|| inner.pending.lock().pop_front());
        item();
    }
}

/// Spawns the global workqueue kernel process and its worker threads.
pub fn init() -> Result<()> {
    let inner = Arc::new(Inner {
        pending: SpinLock::new(VecDeque::new()),
        queued: WaitQueue::new(),
    });
    WORKQ.call_once(|| inner);

    let proc = kproc_create("kworkq", worker_entry, 0)?;
    for _ in 1..WORKERS {
        kthread_add(&proc, "kworkq", worker_entry, 0)?;
    }
    Ok(())
}
