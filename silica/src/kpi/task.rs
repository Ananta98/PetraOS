// SPDX-License-Identifier: GPL-2.0

//! Deferred work: the global workqueue, the `system_wq` counterpart of Linux
//! workqueue semantics.
//!
//! Work items run later in dedicated kernel threads ("process context"), so
//! they may sleep, allocate, or take blocking locks — unlike interrupt-driven
//! top halves. IRQ-safe scheduling is supported: the work queue itself uses an
//! IRQ-disabling spinlock, matching Linux's `schedule_work()` calls from IRQ
//! handlers.
//!
//! A0 scope: one shared global queue, matching the most common Linux pattern.
//! Private per-wheel queues and delayed flush APIs follow with the device
//! core.

use alloc::{boxed::Box, collections::VecDeque, sync::Arc};
use core::time::Duration;
use ostd::sync::{LocalIrqDisabled, SpinLock, WaitQueue};
use spin::Once;

use crate::proc::kthread::{kproc_create, kthread_add};
use crate::uapi::errno::Result;

type WorkFn = Box<dyn FnOnce() + Send>;

struct Inner {
    pending: SpinLock<VecDeque<WorkFn>, LocalIrqDisabled>,
    queued: WaitQueue,
}

/// The global workqueue, created by [`init`] and drained by its worker
/// threads. Safe to hand out because never mutates identity after init.
static WORKQ: Once<Arc<Inner>> = Once::new();

/// Number of worker threads serving the workqueue.
const WORKERS: usize = 4;

/// Schedules a boxed work item to run in workqueue context.
///
/// Must be called after [`init`]; panics otherwise, to surface ordering bugs
/// loudly instead of dropping work silently.
pub fn schedule_work(item: WorkFn) {
    let inner = WORKQ
        .get()
        .cloned()
        .expect("kpi::task: schedule_work before init");
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

/// Schedules a closure to run in workqueue context after `after` elapses,
/// mirroring the delayed-work half of Linux workqueue usage.
pub fn schedule_delayed_work_fn<F>(f: F, after: Duration) -> super::time::KTimer
where
    F: FnOnce() + Send + 'static,
{
    let slot: Arc<SpinLock<Option<WorkFn>, LocalIrqDisabled>> =
        Arc::new(SpinLock::new(Some(Box::new(f))));
    // The timer callback runs in IRQ context and only re-queues; the payload
    // itself keeps running in workqueue (process) context.
    super::time::arm(
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

/// Worker-thread main loop: sleeps until an item is queued, then runs it.
fn worker_entry() {
    let Some(inner) = WORKQ.get().cloned() else {
        return;
    };
    loop {
        // Pop and wake are serialized by the IRQ-disabled lock, so no item
        // can be dropped between waking and running.
        let item = inner.queued.wait_until(|| inner.pending.lock().pop_front());
        item();
    }
}

/// Spawns the workqueue kernel process and its workers.
pub(super) fn init() -> Result<()> {
    let inner = Arc::new(Inner {
        pending: SpinLock::new(VecDeque::new()),
        queued: WaitQueue::new(),
    });
    // The handle must be in place before the threads run, because the threads
    // look the queue up by static handle on entry.
    WORKQ.call_once(|| inner);

    let proc = kproc_create("kpi_workq", worker_entry, 0)?;
    for _ in 1..WORKERS {
        kthread_add(&proc, "kpi_workq", worker_entry, 0)?;
    }
    Ok(())
}
