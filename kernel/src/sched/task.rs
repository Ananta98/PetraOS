//! Task descriptor for the PetraOS scheduler.
//!
//! Defines [`Task`], [`TaskId`], and [`SchedPolicy`] — the fundamental types
//! shared between the CFS and Real-Time run queues.

// ── Scheduling policy ────────────────────────────────────────────────────────

/// The scheduling policy assigned to a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedPolicy {
    /// Normal (CFS) — fair-share CPU time via virtual runtime.
    Normal,
    /// Real-time FIFO — highest-priority runnable task runs to completion
    /// (or until it voluntarily yields/blocks).
    Fifo,
    /// Real-time Round-Robin — like FIFO but with a fixed time slice.
    /// When the slice expires the task is moved to the back of its priority
    /// level.
    RoundRobin,
}

impl SchedPolicy {
    /// Returns `true` if this is a real-time policy.
    #[inline]
    pub fn is_realtime(self) -> bool {
        matches!(self, SchedPolicy::Fifo | SchedPolicy::RoundRobin)
    }
}

// ── Task identifier ──────────────────────────────────────────────────────────

/// Opaque, unique identifier for a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TaskId(pub u64);

// ── Task descriptor ──────────────────────────────────────────────────────────

/// Default time slice for `RoundRobin` tasks, in nanoseconds (10 ms).
pub const DEFAULT_RR_SLICE_NS: u64 = 10_000_000;

/// Weight corresponding to `nice = 0` in the CFS weight table.
///
/// Used to normalise virtual runtime: `vruntime += delta_ns * NICE_0_WEIGHT / weight`.
pub const NICE_0_WEIGHT: u64 = 1024;

/// A lightweight task descriptor consumed by the scheduler subsystem.
///
/// This is intentionally minimal: real PCB / thread state lives in the `proc`
/// module (not yet implemented). The scheduler only needs what is shown here
/// to make pick-next decisions.
#[derive(Debug, Clone)]
pub struct Task {
    /// Unique task identifier.
    pub id: TaskId,
    /// Scheduling policy.
    pub policy: SchedPolicy,
    /// Priority level.
    ///
    /// * For `Normal` tasks this maps to a CFS weight (lower nice → higher weight).
    ///   Stored as a raw weight value (1–88761); use [`nice_to_weight`] to convert.
    /// * For RT tasks (Fifo / RoundRobin) this is an RT priority in `[1, 99]`
    ///   where **99 is the highest** (POSIX convention).
    pub priority: u32,
    /// Accumulated virtual runtime in nanoseconds (CFS only).
    ///
    /// RT tasks leave this at `0` — it is never read by the RT run queue.
    pub vruntime: u64,
    /// Configured time slice in nanoseconds (RoundRobin only).
    pub time_slice_ns: u64,
    /// Remaining nanoseconds in the current time slice (RoundRobin only).
    pub remaining_slice: u64,
}

impl Task {
    /// Create a new `Normal` (CFS) task with a default weight of [`NICE_0_WEIGHT`].
    pub fn new_normal(id: TaskId) -> Self {
        Self {
            id,
            policy: SchedPolicy::Normal,
            priority: NICE_0_WEIGHT as u32,
            vruntime: 0,
            time_slice_ns: 0,
            remaining_slice: 0,
        }
    }

    /// Create a new `Normal` (CFS) task with an explicit weight.
    pub fn new_normal_with_weight(id: TaskId, weight: u32) -> Self {
        Self {
            id,
            policy: SchedPolicy::Normal,
            priority: weight,
            vruntime: 0,
            time_slice_ns: 0,
            remaining_slice: 0,
        }
    }

    /// Create a new FIFO real-time task.
    ///
    /// `rt_priority` must be in `[1, 99]`; higher values are scheduled first.
    pub fn new_fifo(id: TaskId, rt_priority: u32) -> Self {
        Self {
            id,
            policy: SchedPolicy::Fifo,
            priority: rt_priority.clamp(1, 99),
            vruntime: 0,
            time_slice_ns: 0,
            remaining_slice: 0,
        }
    }

    /// Create a new RoundRobin real-time task.
    pub fn new_rr(id: TaskId, rt_priority: u32) -> Self {
        Self {
            id,
            policy: SchedPolicy::RoundRobin,
            priority: rt_priority.clamp(1, 99),
            vruntime: 0,
            time_slice_ns: DEFAULT_RR_SLICE_NS,
            remaining_slice: DEFAULT_RR_SLICE_NS,
        }
    }

    /// Create a new RoundRobin task with an explicit slice duration.
    pub fn new_rr_with_slice(id: TaskId, rt_priority: u32, slice_ns: u64) -> Self {
        Self {
            id,
            policy: SchedPolicy::RoundRobin,
            priority: rt_priority.clamp(1, 99),
            vruntime: 0,
            time_slice_ns: slice_ns,
            remaining_slice: slice_ns,
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Convert a UNIX nice value `[-20, 19]` to a CFS weight.
///
/// Based on the standard Linux weight table (sched/core.c `sched_prio_to_weight`).
pub fn nice_to_weight(nice: i8) -> u32 {
    const WEIGHT_TABLE: [u32; 40] = [
        88761, 71755, 56483, 46273, 36291,
        29154, 23254, 18705, 14949, 11916,
         9548,  7620,  6100,  4904,  3906,
         3121,  2501,  1991,  1586,  1277,
         1024,   820,   655,   526,   423,
          335,   272,   215,   172,   137,
          110,    87,    70,    56,    45,
           36,    29,    23,    18,    15,
    ];
    let index = (nice.clamp(-20, 19) + 20) as usize;
    WEIGHT_TABLE[index]
}
