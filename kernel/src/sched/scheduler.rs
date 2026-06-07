//! Global multi-CPU scheduler.
//!
//! [`GlobalScheduler`] manages up to [`MAX_CPUS`] [`PerCpuScheduler`] instances
//! and provides:
//!
//! * `spawn_task` — adds a task to the least-loaded CPU's run queue.
//! * `schedule`   — triggers a scheduling decision on a specific CPU.
//! * `task_tick`  — advances timer accounting on a specific CPU.
//!
//! # Design note
//!
//! Because PetraOS does not yet have a `proc` module or real context-switching
//! infrastructure, `schedule` merely returns the `TaskId` chosen by the
//! per-CPU scheduler. The caller (future interrupt handler / context-switch
//! path) is responsible for acting on that selection.

use crate::sched::{
    policy::PerCpuScheduler,
    task::{Task, TaskId},
};
use crate::sync::spinlock::Spinlock;

// ── Constants ─────────────────────────────────────────────────────────────────

/// Maximum number of logical CPUs supported by the global scheduler.
pub const MAX_CPUS: usize = 8;

/// Duration of one scheduler tick in nanoseconds.
///
/// Must match the LAPIC timer frequency (100 Hz → 10 ms per tick).
pub const TICK_NS: u64 = 10_000_000;

// ── Kernel-wide scheduler singleton ──────────────────────────────────────────

/// The global, SMP-safe scheduler instance.
///
/// Protected by a [`Spinlock`] so that the LAPIC timer interrupt handler on
/// any CPU can safely call into the scheduler without data races.
pub static GLOBAL_SCHEDULER: Spinlock<GlobalScheduler> =
    Spinlock::new(GlobalScheduler::new());

// ── Convenience free function for interrupt handlers ─────────────────────────

/// Advance timer accounting for `cpu_id` by one tick and return the next task
/// to run on that CPU, if any.
///
/// This is the primary entry-point for the LAPIC timer interrupt handler.
/// It holds the scheduler lock for the minimum time needed.
///
/// Returns `Some(TaskId)` when a runnable task is available, `None` when idle.
pub fn tick_and_schedule(cpu_id: u32) -> Option<TaskId> {
    let mut sched = GLOBAL_SCHEDULER.lock();
    sched.task_tick(cpu_id, TICK_NS);
    sched.schedule(cpu_id)
}

// ── Global scheduler ──────────────────────────────────────────────────────────

/// A global scheduler that coordinates per-CPU run queues.
pub struct GlobalScheduler {
    /// One per-CPU scheduler per logical CPU, up to [`MAX_CPUS`].
    cpus: [Option<PerCpuScheduler>; MAX_CPUS],
    /// Number of CPUs registered via [`GlobalScheduler::register_cpu`].
    cpu_count: usize,
}

impl GlobalScheduler {
    /// Create a global scheduler with no CPUs registered yet.
    pub const fn new() -> Self {
        // SAFETY: Option<PerCpuScheduler> is Copy-initialised to None.
        Self {
            cpus: [const { None }; MAX_CPUS],
            cpu_count: 0,
        }
    }

    // ── CPU management ────────────────────────────────────────────────────────

    /// Register a logical CPU with this scheduler.
    ///
    /// Returns `false` if `cpu_id >= MAX_CPUS` or if the CPU was already
    /// registered.
    pub fn register_cpu(&mut self, cpu_id: u32) -> bool {
        let idx = cpu_id as usize;
        if idx >= MAX_CPUS || self.cpus[idx].is_some() {
            return false;
        }
        self.cpus[idx] = Some(PerCpuScheduler::new(cpu_id));
        self.cpu_count += 1;
        true
    }

    // ── Task lifecycle ────────────────────────────────────────────────────────

    /// Spawn a task onto the CPU with the fewest runnable tasks.
    ///
    /// If `preferred_cpu` is `Some(id)` and that CPU is registered and idle
    /// (or its load is the minimum), the task is placed there.
    ///
    /// Returns the CPU index the task was assigned to, or `None` if no CPUs
    /// are registered.
    pub fn spawn_task(&mut self, task: Task, preferred_cpu: Option<u32>) -> Option<u32> {
        if self.cpu_count == 0 {
            return None;
        }

        // Find the CPU with the minimum runnable count.
        let mut best_cpu: Option<u32> = None;
        let mut best_load = usize::MAX;

        for (idx, slot) in self.cpus.iter().enumerate() {
            let Some(cpu) = slot else { continue };
            let load = cpu.runnable_count();

            // Honour the preferred CPU when it is at minimum load or tied.
            let preferred = preferred_cpu == Some(idx as u32);
            if load < best_load || (load == best_load && preferred) {
                best_load = load;
                best_cpu = Some(idx as u32);
            }
        }

        let cpu_id = best_cpu?;
        let cpu = self.cpus[cpu_id as usize].as_mut()?;
        cpu.add_task(task);
        Some(cpu_id)
    }

    /// Remove a task by `id` from all CPUs (it may only be on one).
    ///
    /// Returns the task and the CPU it was on, if found.
    pub fn remove_task(&mut self, id: TaskId) -> Option<(Task, u32)> {
        for (idx, slot) in self.cpus.iter_mut().enumerate() {
            let Some(cpu) = slot else { continue };
            if let Some(task) = cpu.remove_task(id) {
                return Some((task, idx as u32));
            }
        }
        None
    }

    // ── Scheduling decisions ──────────────────────────────────────────────────

    /// Trigger a scheduling decision on CPU `cpu_id`.
    ///
    /// Returns the `TaskId` of the selected task, or `None` if the CPU is
    /// idle or not registered.
    pub fn schedule(&mut self, cpu_id: u32) -> Option<TaskId> {
        self.cpus[cpu_id as usize].as_mut()?.schedule()
    }

    // ── Timer accounting ──────────────────────────────────────────────────────

    /// Advance timer accounting on CPU `cpu_id` by `delta_ns` nanoseconds.
    pub fn task_tick(&mut self, cpu_id: u32, delta_ns: u64) {
        if let Some(cpu) = self.cpus[cpu_id as usize].as_mut() {
            cpu.task_tick(delta_ns);
        }
    }

    // ── Introspection ─────────────────────────────────────────────────────────

    /// Total runnable tasks across all CPUs.
    pub fn total_runnable(&self) -> usize {
        self.cpus
            .iter()
            .filter_map(|s| s.as_ref())
            .map(|cpu| cpu.runnable_count())
            .sum()
    }

    /// Number of registered CPUs.
    pub fn cpu_count(&self) -> usize {
        self.cpu_count
    }

    /// Returns the per-CPU scheduler for `cpu_id`, or `None` if not registered.
    pub fn cpu(&self, cpu_id: u32) -> Option<&PerCpuScheduler> {
        self.cpus.get(cpu_id as usize)?.as_ref()
    }

    /// Returns a mutable reference to the per-CPU scheduler for `cpu_id`, or `None` if not registered.
    pub fn cpu_mut(&mut self, cpu_id: u32) -> Option<&mut PerCpuScheduler> {
        self.cpus.get_mut(cpu_id as usize)?.as_mut()
    }

    /// Directly sets the currently running task for a specific CPU.
    pub fn set_running_task(&mut self, cpu_id: u32, task: Task) {
        if let Some(cpu) = self.cpu_mut(cpu_id) {
            cpu.running = Some(task);
        }
    }
}
