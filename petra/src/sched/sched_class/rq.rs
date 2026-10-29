// SPDX-License-Identifier: GPL-2.0

//! Composite per-CPU runqueue.
//!
//! Orchestrates the multi-class scheduling hierarchy:
//! 1. Real-Time (`SCHED_FIFO`, `SCHED_RR` via [`RtRunQueue`])
//! 2. Fair (EEVDF via [`EevdfRunQueue`])
//! 3. CPU Idle Task

use alloc::sync::Arc;
use ostd::task::Task;
use ostd::task::scheduler::UpdateFlags;

use super::SchedEntity;
use super::eevdf::EevdfRunQueue;
use super::rt::RtRunQueue;
use crate::sched::now;
use crate::sched::stats::CpuStats;
use crate::uapi::sched::SCHED_RR;

/// Runnable task together with attached scheduling entity and arrival tick.
#[derive(Clone)]
pub struct SchedEntry {
    /// The task.
    pub task: Arc<Task>,
    /// Unified scheduling state.
    pub entity: Arc<SchedEntity>,
    /// Arrival tick on this runqueue.
    pub arrival: u64,
    /// Whether this is the CPU idle task.
    pub is_idle: bool,
}

impl SchedEntry {
    /// Creates a standard task entry.
    pub fn new(task: Arc<Task>, entity: Arc<SchedEntity>, arrival: u64) -> Self {
        Self {
            task,
            entity,
            arrival,
            is_idle: false,
        }
    }

    /// Creates an idle task entry.
    pub fn idle(task: Arc<Task>, entity: Arc<SchedEntity>) -> Self {
        Self {
            task,
            entity,
            arrival: now(),
            is_idle: true,
        }
    }

    /// Returns the underlying task.
    pub fn into_task(self) -> Arc<Task> {
        self.task
    }
}

/// Composite per-CPU runqueue.
pub struct RunQueue {
    /// Task currently running on this CPU.
    current: Option<SchedEntry>,
    /// Real-time scheduling class queue.
    rt: RtRunQueue,
    /// EEVDF fair scheduling class queue.
    eevdf: EevdfRunQueue,
    /// Idle task entry for this CPU.
    idle: Option<SchedEntry>,
    /// Tick when the running task was last charged.
    last_update: u64,
    /// Last update event flag.
    last_flags: UpdateFlags,
    /// Per-CPU statistics counters.
    stats: CpuStats,
}

impl RunQueue {
    /// Creates an empty per-CPU runqueue.
    pub fn new() -> Self {
        Self {
            current: None,
            rt: RtRunQueue::new(),
            eevdf: EevdfRunQueue::new(),
            idle: None,
            last_update: 0,
            last_flags: UpdateFlags::Tick,
            stats: CpuStats::new(),
        }
    }

    /// Returns the task currently running on this CPU, if any.
    pub fn current_task(&self) -> Option<&Arc<Task>> {
        self.current.as_ref().map(|entry| &entry.task)
    }

    /// Returns the scheduling entity of the currently running task.
    pub fn current_entity(&self) -> Option<&Arc<SchedEntity>> {
        self.current.as_ref().map(|entry| &entry.entity)
    }

    /// Total number of runnable tasks (excluding the idle task).
    pub fn nr_running(&self) -> usize {
        self.rt.nr_running()
            + self.eevdf.nr_running()
            + usize::from(self.current.as_ref().is_some_and(|e| !e.is_idle))
    }

    /// Number of queued tasks waiting for the CPU.
    pub fn nr_queued(&self) -> usize {
        self.rt.nr_running() + self.eevdf.nr_queued()
    }

    /// Returns a reference to the CPU statistics counters.
    pub fn stats(&self) -> &CpuStats {
        &self.stats
    }

    /// Returns true if the CPU is running its idle task.
    pub fn is_idling(&self) -> bool {
        self.current.as_ref().is_some_and(|e| e.is_idle)
    }

    /// Returns true if idling can begin (currently not running idle task).
    pub fn can_idle(&self) -> bool {
        !self.is_idling()
    }

    /// Charges the running task for the time elapsed since the last update.
    pub fn charge(&mut self) {
        let current_tick = now();
        let delta_ticks = current_tick.saturating_sub(self.last_update);
        self.last_update = current_tick;
        if delta_ticks == 0 {
            return;
        }

        let delta_ns = delta_ticks.saturating_mul(crate::sched::NSEC_PER_TICK);
        let Some(current) = &self.current else {
            return;
        };

        current.entity.charge_exec(delta_ns);

        if current.is_idle {
            self.stats.idled(delta_ns);
        } else if current.entity.is_rt() {
            self.stats.ran(delta_ns);
        } else {
            self.stats.ran(delta_ns);
            self.eevdf.charge_current(delta_ns);
        }
    }

    /// Places a task into its appropriate scheduling class.
    pub fn place(&mut self, entry: SchedEntry, is_new: bool) {
        if entry.entity.is_rt() {
            self.rt.enqueue(entry, false);
        } else {
            self.eevdf.place(entry, is_new);
        }
    }

    /// Checks if a newly arrived entity should preempt the currently running task.
    pub fn should_preempt_entity(&self, entity: &SchedEntity) -> bool {
        let Some(current) = &self.current else {
            // Nothing is running in task context (we are in bootstrap context,
            // which cannot be switched back to and must not be interrupted).
            return false;
        };
        if current.is_idle {
            return true;
        }

        if entity.is_rt() {
            // Any RT task preempts normal fair tasks.
            if !current.entity.is_rt() {
                return true;
            }
            // Higher RT priority preempts lower RT priority.
            return entity.rt.priority() > current.entity.rt.priority();
        }

        // Fair tasks cannot preempt RT tasks.
        if current.entity.is_rt() {
            return false;
        }

        // Within Fair class: eligible and earlier deadline.
        self.eevdf.is_eligible(&entity.fair)
            && entity.fair.deadline() < current.entity.fair.deadline()
    }

    /// Determines whether the running task should be switched away from.
    pub fn should_switch(&mut self) -> bool {
        let Some(current) = &self.current else {
            return false;
        };
        if current.is_idle {
            return self.nr_running() > 0;
        }

        if current.entity.is_rt() {
            // Handle round-robin quantum expiration and higher-priority arrivals.
            let delta_ns = crate::sched::NSEC_PER_TICK;
            return self.rt.tick(current, delta_ns);
        }

        // If an RT task has arrived, immediately preempt Fair class.
        if self.rt.has_tasks() {
            return true;
        }

        // Fair class: check if best waiting candidate displaces current.
        self.eevdf
            .best()
            .is_some_and(|best| self.eevdf.wins_over_current(best, false))
    }

    /// Selects the next task to run based on class priority: RT > EEVDF > Idle.
    pub fn pick_next(&mut self, yielding: bool) -> Option<SchedEntry> {
        // Priority 1: Real-time class
        if self.rt.has_tasks()
            && let Some(rt_best) = self.rt.pick_next()
        {
            let candidate = rt_best.clone();
            let should_switch = match &self.current {
                None => true,
                Some(curr) if curr.is_idle || !curr.entity.is_rt() => true,
                Some(curr) => {
                    yielding
                        || candidate.entity.rt.priority() > curr.entity.rt.priority()
                        || (candidate.entity.rt.priority() == curr.entity.rt.priority()
                            && curr.entity.policy() == SCHED_RR
                            && curr.entity.rt.time_slice_ns() == 0)
                }
            };
            if should_switch {
                return Some(candidate);
            }
        }

        // If current is RT and no higher RT exists, RT keeps running.
        if let Some(curr) = &self.current
            && curr.entity.is_rt()
            && !yielding
        {
            return None;
        }

        // Priority 2: EEVDF Fair class
        if let Some(best) = self.eevdf.best()
            && self.eevdf.wins_over_current(best, yielding)
        {
            return Some(best.clone());
        }

        None
    }

    /// Installs `next` as the running task on this CPU.
    pub fn install(&mut self, next: SchedEntry, from_queue: bool) {
        let next = if from_queue {
            if next.entity.is_rt() {
                self.rt
                    .dequeue(&next.task)
                    .expect("RT task must be in runqueue")
            } else {
                self.eevdf.extract(&next)
            }
        } else {
            next
        };

        // Switch out current task
        if let Some(mut current) = self.current.replace(next) {
            self.stats
                .switched_out(self.last_flags == UpdateFlags::Yield);
            crate::sched::stats::global().switched();

            if !current.is_idle {
                current.arrival = now();
                if current.entity.is_rt() {
                    if current.entity.policy() == SCHED_RR {
                        current.entity.rt.reset_time_slice();
                    }
                    if self.last_flags == UpdateFlags::Yield {
                        self.rt.yield_task(&current);
                    } else {
                        self.rt.enqueue(current, false);
                    }
                } else {
                    self.eevdf.insert(current, false);
                }
            }
        }

        let running = self.current.as_ref().expect("current task just installed");
        if !running.is_idle && !running.entity.is_rt() {
            self.eevdf.set_current(self.current.clone());
        } else {
            self.eevdf.set_current(None);
        }

        running.entity.dispatched(now(), running.arrival);
    }

    /// Dequeues the currently running task (e.g. on sleep, wait, or exit).
    pub fn dequeue_current(&mut self) -> Option<SchedEntry> {
        if self.is_idling() {
            return None;
        }
        let current = self.current.take()?;
        current.task.schedule_info().cpu.set_to_none();

        if !current.entity.is_rt() {
            self.eevdf.set_current(None);
            self.eevdf.remember_lag(&current.entity.fair);
            self.eevdf.unaccount(&current.entity.fair);
        }

        if self.last_flags == UpdateFlags::Exit {
            crate::sched::adapter::forget_entity(&current.task);
        }

        Some(current)
    }

    /// Starts idling this CPU if there is no runnable work.
    pub fn start_idling(&mut self) -> bool {
        if self.is_idling() {
            return true;
        }
        if self.idle.is_none() {
            let Some(idle) = crate::sched::adapter::idle_entry() else {
                return false;
            };
            self.idle = Some(idle);
        }
        let idle = self.idle.clone().expect("idle entry initialized");
        self.install(idle, false);
        true
    }

    /// Records the update flags for the current decision.
    pub fn set_last_flags(&mut self, flags: UpdateFlags) {
        self.last_flags = flags;
    }

    /// Returns the last update flags.
    pub fn last_flags(&self) -> UpdateFlags {
        self.last_flags
    }
}

impl Default for RunQueue {
    fn default() -> Self {
        Self::new()
    }
}
