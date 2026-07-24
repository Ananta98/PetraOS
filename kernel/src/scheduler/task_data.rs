use crate::proc::pid_table::Pid;
use crate::proc::tid_table::Tid;
use crate::scheduler::SchedClass;
use crate::scheduler::nice::NiceWeight;
use core::sync::atomic::{AtomicU64, Ordering};
use ostd::task::Task;

/// Per-task scheduling metadata attached to every `ostd::task::Task`.
pub struct TaskData {
    /// Scheduling class and parameters.
    pub class: SchedClass,
    /// Accumulated virtual runtime (nanoseconds, CFS bookkeeping).
    pub vruntime: AtomicU64,
    /// Infinity Scheduler: Exponential Moving Average for execution slices.
    pub ema: AtomicU64,
    /// Last time this task was dequeued (vtime).
    pub last_dequeue_vtime: AtomicU64,
    /// Owning process identifier.
    pub pid: Pid,
    /// This thread's unique identifier.
    pub tid: Tid,
}

impl TaskData {
    /// Create `TaskData` with an explicit scheduling class, `Pid`, and `Tid`.
    pub fn new(class: SchedClass, pid: Pid, tid: Tid) -> Self {
        Self {
            class,
            vruntime: AtomicU64::new(0),
            ema: AtomicU64::new(0),
            last_dequeue_vtime: AtomicU64::new(0),
            pid,
            tid,
        }
    }

    /// Extract `(SchedClass, vruntime)` from an `ostd::task::Task` reference.
    pub fn sched_data(task: &Task) -> (SchedClass, u64) {
        if let Some(data) = task.data().downcast_ref::<Self>() {
            (data.class, data.vruntime.load(Ordering::Relaxed))
        } else {
            (
                SchedClass::Fair {
                    nice: NiceWeight::new(0),
                },
                0,
            )
        }
    }

    /// Update the virtual runtime of an `ostd::task::Task`.
    pub fn set_vruntime(task: &Task, vruntime: u64) {
        if let Some(data) = task.data().downcast_ref::<Self>() {
            data.vruntime.store(vruntime, Ordering::Relaxed);
        }
    }

    /// Calculate the EEVDF virtual deadline for a task.
    pub fn deadline(vruntime: u64, class: SchedClass, task: Option<&Task>) -> u64 {
        let mut weight = class.weight();
        if let Some(task) = task {
            if let Some(data) = task.data().downcast_ref::<Self>() {
                let ema = data.ema.load(Ordering::Relaxed);
                let ema_pct = (ema * 100 / 2_000_000).min(100);
                let weight_factor = 100 - ema_pct * 75 / 100;
                weight = weight * 100 / weight_factor.max(1);
            }
        }
        vruntime + 1024_000 / weight.max(1)
    }
}
