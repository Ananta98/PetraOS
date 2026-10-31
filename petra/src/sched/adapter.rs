// SPDX-License-Identifier: GPL-2.0

//! Scheduler adapter implementing OSTD `Scheduler` and `LocalRunQueue` traits.
//!
//! Provides runqueue binding, task registry, and idle loop coordination across CPUs.

use alloc::{boxed::Box, collections::BTreeMap, sync::Arc};
use core::sync::atomic::{AtomicUsize, Ordering};

use ostd::{
    cpu::{CpuId, PinCurrentCpu, num_cpus},
    sync::SpinLock,
    task::{
        Task, TaskOptions, disable_preempt, halt_cpu,
        scheduler::{
            EnqueueFlags, LocalRunQueue, Scheduler, UpdateFlags, enable_preemption_on_cpu,
            inject_scheduler,
        },
    },
    timer,
};
use spin::Once;

use super::{
    now,
    sched_class::{RunQueue, SchedEntity, SchedEntry, eevdf::BASE_SLICE_NS},
    stats,
};

/// Global sequencer for unique tie-breaking order.
static NEXT_SEQ: AtomicUsize = AtomicUsize::new(0);

/// Primary scheduler implementation driven by OSTD.
pub struct EevdfScheduler {
    /// Per-CPU runqueues.
    rqs: Box<[SpinLock<RunQueue>]>,
    /// Global registry mapping task addresses to unified scheduling entities.
    entities: SpinLock<BTreeMap<usize, Arc<SchedEntity>>>,
}

impl EevdfScheduler {
    /// Creates a scheduler instance with per-CPU runqueues.
    fn new() -> Self {
        let rqs = (0..num_cpus())
            .map(|_| SpinLock::new(RunQueue::new()))
            .collect();
        Self {
            rqs,
            entities: SpinLock::new(BTreeMap::new()),
        }
    }

    /// Returns a reference to the spinlock guarding `cpu`'s runqueue.
    fn rq(&self, cpu: CpuId) -> &SpinLock<RunQueue> {
        &self.rqs[u32::from(cpu) as usize]
    }

    /// Returns the most suitable target CPU for an entity among allowed CPUs.
    fn select_target_cpu(&self, entity: &SchedEntity, current_cpu: CpuId) -> CpuId {
        let ncpus = num_cpus();
        if ncpus <= 1 {
            return current_cpu;
        }

        let affinity = entity.affinity();
        let current_id = u32::from(current_cpu) as usize;
        if affinity.is_set(current_id)
            && self.rq(current_cpu).disable_irq().lock().nr_running() == 0
        {
            return current_cpu;
        }

        let mut best_cpu = current_cpu;
        let mut min_load = usize::MAX;

        for cpu_idx in 0..ncpus {
            if !affinity.is_set(cpu_idx) {
                continue;
            }
            if let Ok(cpu) = cpu_idx.try_into() {
                let load = self.rq(cpu).disable_irq().lock().nr_running();
                if load == 0 {
                    return cpu;
                }
                if load < min_load {
                    min_load = load;
                    best_cpu = cpu;
                }
            }
        }

        best_cpu
    }

    /// Enqueues a task onto a target CPU, returning the CPU ID if preemption is requested.
    fn enqueue_task(
        &self,
        runnable: Arc<Task>,
        entity: Arc<SchedEntity>,
        flags: EnqueueFlags,
    ) -> Option<CpuId> {
        let current_cpu = disable_preempt().current_cpu();
        let (still_queued, target_cpu) = match runnable.schedule_info().cpu.get() {
            Some(cpu) => {
                if !entity.affinity().is_set(u32::from(cpu) as usize) {
                    let migrated = self.select_target_cpu(&entity, current_cpu);
                    runnable.schedule_info().cpu.set_anyway(migrated);
                    entity.nr_migrations.fetch_add(1, Ordering::Relaxed);
                    (false, migrated)
                } else {
                    (true, cpu)
                }
            }
            None => {
                let target = self.select_target_cpu(&entity, current_cpu);
                let _ = runnable.schedule_info().cpu.set_if_is_none(target);
                (
                    false,
                    runnable.schedule_info().cpu.get().unwrap_or(current_cpu),
                )
            }
        };

        debug_assert!(
            flags != EnqueueFlags::Spawn || !still_queued,
            "newly created tasks must not already be marked as queued"
        );

        let mut rq = self.rq(target_cpu).disable_irq().lock();
        if still_queued
            && runnable
                .schedule_info()
                .cpu
                .set_if_is_none(target_cpu)
                .is_err()
        {
            // Redundant wakeup race; task already queued.
            return None;
        }

        let is_new = !entity.fair.is_placed();
        rq.place(SchedEntry::new(runnable, entity.clone(), now()), is_new);

        rq.should_preempt_entity(&entity).then_some(target_cpu)
    }
}

impl Scheduler<Task> for EevdfScheduler {
    fn enqueue(&self, runnable: Arc<Task>, flags: EnqueueFlags) -> Option<CpuId> {
        let entity = entity_for(&runnable);
        self.enqueue_task(runnable, entity, flags)
    }

    fn local_rq_with(&self, f: &mut dyn FnMut(&dyn LocalRunQueue)) {
        let guard = disable_preempt();
        let rq = self.rq(guard.current_cpu()).disable_irq().lock();
        f(&*rq);
    }

    fn mut_local_rq_with(&self, f: &mut dyn FnMut(&mut dyn LocalRunQueue)) {
        let guard = disable_preempt();
        let mut rq = self.rq(guard.current_cpu()).disable_irq().lock();
        f(&mut *rq);
    }
}

impl LocalRunQueue for RunQueue {
    fn current(&self) -> Option<&Arc<Task>> {
        self.current_task()
    }

    fn update_current(&mut self, flags: UpdateFlags) -> bool {
        self.set_last_flags(flags);
        self.charge();
        match flags {
            UpdateFlags::Tick => self.should_switch(),
            UpdateFlags::Yield => {
                if self.current_task().is_none() {
                    true
                } else {
                    self.pick_next(true).is_some()
                }
            }
            UpdateFlags::Wait | UpdateFlags::Exit => self.nr_queued() > 0 || self.can_idle(),
        }
    }

    fn try_pick_next(&mut self) -> Option<&Arc<Task>> {
        let yielding = self.last_flags() == UpdateFlags::Yield;
        if let Some(next) = self.pick_next(yielding) {
            self.install(next, true);
            return self.current_task();
        }

        let running = self.current_task().map(Arc::as_ptr);
        if running.is_none() {
            self.start_idling();
        }

        match self.current_task() {
            Some(current) if Some(Arc::as_ptr(current)) != running => Some(current),
            _ => None,
        }
    }

    fn dequeue_current(&mut self) -> Option<Arc<Task>> {
        RunQueue::dequeue_current(self).map(|entry| entry.into_task())
    }
}

static SCHEDULER: Once<EevdfScheduler> = Once::new();

/// Initializes the global kernel scheduler.
pub fn init() {
    SCHEDULER.call_once(EevdfScheduler::new);
    inject_scheduler(SCHEDULER.get().expect("scheduler must be initialized"));
}

/// Enables timer callbacks and preemption on the calling CPU.
pub fn init_on_cpu() {
    enable_preemption_on_cpu();
    timer::register_callback_on_cpu(|| {
        stats::on_loadavg_tick(nr_running_on(disable_preempt().current_cpu()));
        stats::global().ticked();
    });
}

/// Builds a task with nice value `nice` without making it runnable immediately.
pub fn build(options: TaskOptions, nice: i8) -> Result<Arc<Task>, ostd::Error> {
    let task = Arc::new(options.build()?);
    let seq = NEXT_SEQ.fetch_add(1, Ordering::Relaxed);
    remember_entity(&task, SchedEntity::new_fair(nice, BASE_SLICE_NS, seq));
    Ok(task)
}

/// Updates scheduling parameters (policy, priority, nice) for a live task.
pub fn set_task_sched_params(task: &Task, policy: i32, priority: i32, nice: i8) {
    if let Some(entity) = entity_of(task) {
        entity.set_params(policy, priority, nice);
    }
}

/// Returns the scheduling entity associated with `task`, if registered.
pub fn entity_of(task: &Task) -> Option<Arc<SchedEntity>> {
    let key = key_of(task);
    SCHEDULER
        .get()?
        .entities
        .disable_irq()
        .lock()
        .get(&key)
        .cloned()
}

/// Returns the number of runnable tasks on `cpu`, excluding idle.
pub fn nr_running_on(cpu: CpuId) -> usize {
    scheduler().rq(cpu).disable_irq().lock().nr_running()
}

/// Returns a snapshot of CPU scheduling statistics for `cpu`.
pub fn cpu_stats_on(cpu: CpuId) -> Option<stats::CpuStatsSnapshot> {
    SCHEDULER
        .get()
        .map(|s| s.rq(cpu).disable_irq().lock().stats().snapshot())
}

fn scheduler() -> &'static EevdfScheduler {
    SCHEDULER.get().expect("scheduler has not been initialized")
}

fn key_of(task: &Task) -> usize {
    task as *const Task as usize
}

fn remember_entity(task: &Task, entity: Arc<SchedEntity>) {
    scheduler()
        .entities
        .disable_irq()
        .lock()
        .insert(key_of(task), entity);
    stats::global().entity_created();
}

fn entity_for(task: &Task) -> Arc<SchedEntity> {
    let key = key_of(task);
    let mut entities = scheduler().entities.disable_irq().lock();
    if let Some(entity) = entities.get(&key) {
        return Arc::clone(entity);
    }
    let seq = NEXT_SEQ.fetch_add(1, Ordering::Relaxed);
    let entity = SchedEntity::new_fair(0, BASE_SLICE_NS, seq);
    entities.insert(key, Arc::clone(&entity));
    stats::global().entity_created();
    entity
}

/// Unregisters the scheduling entity of an exiting task.
pub(super) fn forget_entity(task: &Task) {
    let Some(scheduler) = SCHEDULER.get() else {
        return;
    };
    scheduler
        .entities
        .disable_irq()
        .lock()
        .remove(&key_of(task));
}

fn idle_task() {
    loop {
        halt_cpu();
    }
}

/// Allocates an idle entry for the current CPU.
pub(super) fn idle_entry() -> Option<SchedEntry> {
    let task = match TaskOptions::new(idle_task).build() {
        Ok(task) => Arc::new(task),
        Err(err) => {
            ostd::warn!("cannot create idle task: {err:?}");
            return None;
        }
    };
    Some(SchedEntry::idle(task, SchedEntity::idle()))
}
