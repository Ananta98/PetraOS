//! Tasklet Infrastructure built on top of SoftIRQ `SoftIrqVector::Tasklet`.

use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use ostd::sync::SpinLock;

use super::soft::{SoftIrqVector, raise_softirq};

/// A deferred, one-shot work item scheduled through the `Tasklet` softirq vector.
pub struct Tasklet {
    func: Arc<dyn Fn() + Send + Sync + 'static>,
    scheduled: AtomicBool,
}

impl Tasklet {
    /// Create a new [`Tasklet`] backed by `func`.
    pub fn new(func: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            func: Arc::new(func),
            scheduled: AtomicBool::new(false),
        }
    }

    /// Enqueue this tasklet for execution at the next [`do_softirq`].
    pub fn schedule(&self) {
        if !self.scheduled.swap(true, Ordering::AcqRel) {
            raise_softirq(SoftIrqVector::Tasklet);
        }
    }

    /// Execute the tasklet and mark it as no longer scheduled.
    pub fn run(&self) {
        self.scheduled.store(false, Ordering::Release);
        (self.func)();
    }
}

/// Global queue of pending [`Tasklet`]s to execute under the `Tasklet` vector.
static TASKLET_QUEUE: SpinLock<Vec<Arc<Tasklet>>> = SpinLock::new(Vec::new());

/// Submit an `Arc<Tasklet>` for deferred execution.
pub fn schedule_tasklet(tasklet: Arc<Tasklet>) {
    tasklet.schedule();
    TASKLET_QUEUE.lock().push(tasklet);
}

/// Bottom-half handler installed for [`SoftIrqVector::Tasklet`].
pub fn run_tasklets() {
    let items: Vec<Arc<Tasklet>> = {
        let mut queue = TASKLET_QUEUE.lock();
        core::mem::take(&mut *queue)
    };
    for tasklet in items {
        tasklet.run();
    }
}
