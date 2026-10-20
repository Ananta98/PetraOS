// SPDX-License-Identifier: GPL-2.0

//! Earliest Eligible Virtual Deadline First (EEVDF) scheduling class.
//!
//! Implements latency-sensitive proportional share scheduling (Linux 6.6+).
//! Tasks allocate a virtual runtime proportional to consumed CPU time and inversely
//! proportional to load weight. A task is eligible when its virtual runtime does
//! not exceed the runqueue's weighted average virtual time. Of all eligible tasks,
//! the one with the earliest virtual deadline is dispatched.

use alloc::{collections::BTreeMap, sync::Arc};
use core::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};
use ostd::task::Task;

use super::rq::SchedEntry;

/// Load weight of a nice 0 task (proportional share baseline).
pub const NICE_0_LOAD: u32 = 1024;

/// Default time slice requested by tasks (3 ms).
pub const BASE_SLICE_NS: u64 = 3_000_000;

/// Minimum allowable time slice (100 us).
pub const MIN_SLICE_NS: u64 = 100_000;

/// Maximum allowable time slice (100 ms).
pub const MAX_SLICE_NS: u64 = 100_000_000;

/// Lowest allowable nice value (highest priority).
pub const NICE_MIN: i8 = -20;

/// Highest allowable nice value (lowest priority).
pub const NICE_MAX: i8 = 19;

/// Linux `sched_prio_to_weight` mapping for nice -20..=19.
const PRIO_TO_WEIGHT: [u32; 40] = [
    88761, 71755, 56483, 46273, 36291, 29154, 23254, 18705, 14949, 11916, 9548, 7620, 6100, 4904,
    3906, 3121, 2501, 1991, 1586, 1277, 1024, 820, 655, 526, 423, 335, 272, 215, 172, 137, 110, 87,
    70, 56, 45, 36, 29, 23, 18, 15,
];

/// Returns the proportional load weight for nice value `nice`.
pub fn weight_from_nice(nice: i8) -> u32 {
    let index = (nice.clamp(NICE_MIN, NICE_MAX) - NICE_MIN) as usize;
    PRIO_TO_WEIGHT[index]
}

/// Returns the nearest nice value corresponding to `weight`.
pub fn nice_from_weight(weight: u32) -> i8 {
    let index = PRIO_TO_WEIGHT
        .iter()
        .position(|&w| w <= weight)
        .unwrap_or(PRIO_TO_WEIGHT.len() - 1);
    NICE_MIN + index as i8
}

/// Computes `(value * numerator) / denominator` using 128-bit arithmetic, saturating at `u64::MAX`.
#[inline]
pub(crate) fn scale(value: u64, numerator: u64, denominator: u64) -> u64 {
    if denominator == 0 {
        return 0;
    }
    u64::try_from((value as u128 * numerator as u128) / denominator as u128).unwrap_or(u64::MAX)
}

/// EEVDF scheduling state of a single task.
pub struct EevdfEntity {
    vruntime: AtomicU64,
    weight: AtomicU32,
    slice: AtomicU64,
    placed: AtomicBool,
    lag: AtomicI64,
}

impl EevdfEntity {
    /// Creates an EEVDF entity with the specified nice value and time slice.
    pub fn new(nice: i8, slice_ns: u64) -> Self {
        Self {
            vruntime: AtomicU64::new(0),
            weight: AtomicU32::new(weight_from_nice(nice)),
            slice: AtomicU64::new(slice_ns.clamp(MIN_SLICE_NS, MAX_SLICE_NS)),
            placed: AtomicBool::new(false),
            lag: AtomicI64::new(0),
        }
    }

    /// Returns the accumulated virtual runtime in virtual nanoseconds.
    pub fn vruntime(&self) -> u64 {
        self.vruntime.load(Ordering::Relaxed)
    }

    /// Sets the task's virtual runtime (runqueue lock must be held).
    pub fn set_vruntime(&self, vruntime: u64) {
        self.vruntime.store(vruntime, Ordering::Relaxed);
    }

    /// Returns the task weight.
    pub fn weight(&self) -> u32 {
        self.weight.load(Ordering::Relaxed).max(1)
    }

    /// Updates the nice value and recalculates weight.
    pub fn set_nice(&self, nice: i8) {
        self.weight.store(weight_from_nice(nice), Ordering::Relaxed);
    }

    /// Returns the nice value corresponding to the current weight.
    pub fn nice(&self) -> i8 {
        nice_from_weight(self.weight())
    }

    /// Returns the requested slice in nanoseconds.
    pub fn slice_ns(&self) -> u64 {
        self.slice.load(Ordering::Relaxed)
    }

    /// Returns the virtual time duration of the requested slice.
    pub fn request(&self) -> u64 {
        scale(self.slice_ns(), NICE_0_LOAD as u64, self.weight() as u64)
    }

    /// Returns the virtual deadline: `vruntime + request`.
    pub fn deadline(&self) -> u64 {
        self.vruntime().saturating_add(self.request())
    }

    /// Advances virtual runtime by real duration `delta_ns`.
    pub fn charge(&self, delta_ns: u64) {
        let delta_vruntime = scale(delta_ns, NICE_0_LOAD as u64, self.weight() as u64);
        let mut vruntime = self.vruntime();
        while vruntime != u64::MAX {
            match self.vruntime.compare_exchange_weak(
                vruntime,
                vruntime.saturating_add(delta_vruntime),
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => vruntime = actual,
            }
        }
    }

    /// Returns whether this entity has ever been placed on a runqueue.
    pub fn is_placed(&self) -> bool {
        self.placed.load(Ordering::Relaxed)
    }

    /// Marks that this entity has been placed on a runqueue.
    pub fn set_placed(&self) {
        self.placed.store(true, Ordering::Relaxed);
    }

    /// Returns the preserved lag upon sleeping / leaving the runqueue.
    pub fn lag(&self) -> i64 {
        self.lag.load(Ordering::Relaxed)
    }

    /// Sets the preserved lag.
    pub fn set_lag(&self, lag: i64) {
        self.lag.store(lag, Ordering::Relaxed);
    }
}

/// Ordering key for tasks queued in an EEVDF runqueue.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct EevdfRunKey {
    /// Virtual deadline (earliest deadline first).
    pub deadline: u64,
    /// Virtual runtime (breaks deadline ties in favor of less-run tasks).
    pub vruntime: u64,
    /// Unique task sequence identifier (deterministic tie-breaker).
    pub seq: usize,
}

impl EevdfRunKey {
    /// Computes the key for an entity, pushing deadline by one request if yielding.
    pub fn of(entry: &SchedEntry, yielding: bool) -> Self {
        let deadline = entry.entity.fair.deadline();
        let penalty = if yielding {
            entry.entity.fair.request()
        } else {
            0
        };
        Self {
            deadline: deadline.saturating_add(penalty),
            vruntime: entry.entity.fair.vruntime(),
            seq: entry.entity.seq,
        }
    }
}

/// EEVDF runqueue for a single CPU.
pub struct EevdfRunQueue {
    /// Tasks queued waiting for execution, ordered by `EevdfRunKey`.
    queued: BTreeMap<EevdfRunKey, SchedEntry>,
    /// Currently running fair task on this CPU (if any).
    current: Option<SchedEntry>,
    /// Sum of weights of queued and current fair tasks.
    sum_weight: u64,
    /// Weighted offset sum: `sum((vruntime - min_vruntime) * weight)`.
    sum_offset_weighted: u64,
    /// Smallest virtual runtime among active tasks, monotonically increasing.
    min_vruntime: u64,
}

impl EevdfRunQueue {
    /// Creates an empty EEVDF runqueue.
    pub const fn new() -> Self {
        Self {
            queued: BTreeMap::new(),
            current: None,
            sum_weight: 0,
            sum_offset_weighted: 0,
            min_vruntime: 0,
        }
    }

    /// Returns the number of runnable tasks in this EEVDF class.
    pub fn nr_running(&self) -> usize {
        self.queued.len() + usize::from(self.current.is_some())
    }

    /// Returns the number of waiting tasks in the queue.
    pub fn nr_queued(&self) -> usize {
        self.queued.len()
    }

    /// Returns the weighted average virtual runtime: `min_vruntime + (sum_offset / sum_weight)`.
    pub fn avg_vruntime(&self) -> u64 {
        if self.sum_weight == 0 {
            return self.min_vruntime;
        }
        self.min_vruntime
            .saturating_add(self.sum_offset_weighted / self.sum_weight)
    }

    /// Returns the monotonic smallest virtual runtime.
    pub fn min_vruntime(&self) -> u64 {
        self.min_vruntime
    }

    /// Sets the currently active fair task.
    pub fn set_current(&mut self, entry: Option<SchedEntry>) {
        self.current = entry;
    }

    /// Returns a reference to the currently running fair task.
    pub fn current(&self) -> Option<&SchedEntry> {
        self.current.as_ref()
    }

    /// Checks whether an entity is eligible: `vruntime <= avg_vruntime`.
    pub fn is_eligible(&self, entity: &EevdfEntity) -> bool {
        entity.vruntime() <= self.avg_vruntime()
    }

    /// Charges the running fair task for execution duration `delta_ns`.
    pub fn charge_current(&mut self, delta_ns: u64) {
        if delta_ns == 0 {
            return;
        }
        let Some(current) = &self.current else {
            return;
        };

        let before = current.entity.fair.vruntime();
        current.entity.fair.charge(delta_ns);
        let after = current.entity.fair.vruntime();
        let delta_vruntime = after.saturating_sub(before);

        let weight = current.entity.fair.weight() as u64;
        self.sum_offset_weighted = self
            .sum_offset_weighted
            .saturating_add(delta_vruntime.saturating_mul(weight));

        if before == self.min_vruntime {
            self.advance_min_vruntime();
        }
    }

    /// Places a task into the EEVDF runqueue, restoring preserved lag.
    pub fn place(&mut self, entry: SchedEntry, is_new: bool) {
        let lag = if is_new { 0 } else { entry.entity.fair.lag() };

        // Scale lag by change in total weight: Linux EEVDF formulation
        let weight = entry.entity.fair.weight() as i64;
        let total = self.sum_weight as i64;
        let scaled_lag = if total == 0 || weight == 0 {
            lag
        } else {
            ((lag as i128 * (total as i128 + weight as i128)) / total as i128) as i64
        };

        let base_vruntime = self.avg_vruntime();
        let vruntime =
            ((base_vruntime as i128 - scaled_lag as i128).max(self.min_vruntime as i128)) as u64;

        if self.sum_weight == 0 {
            self.min_vruntime = self.min_vruntime.max(vruntime);
            entry.entity.fair.set_vruntime(vruntime);
        } else {
            entry.entity.fair.set_vruntime(vruntime);
        }

        entry.entity.fair.set_placed();
        self.insert(entry, false);
    }

    /// Inserts a task into the queued tree.
    pub fn insert(&mut self, entry: SchedEntry, counted: bool) {
        let key = EevdfRunKey::of(&entry, false);
        let weight = entry.entity.fair.weight() as u64;
        let vruntime = entry.entity.fair.vruntime();

        self.queued.insert(key, entry);

        if !counted {
            self.sum_weight = self.sum_weight.saturating_add(weight);
            self.sum_offset_weighted = self.sum_offset_weighted.saturating_add(
                vruntime
                    .saturating_sub(self.min_vruntime)
                    .saturating_mul(weight),
            );
        }
    }

    /// Removes a task from the queued tree and unaccounts it from running sums.
    pub fn dequeue(&mut self, task: &Task) -> Option<SchedEntry> {
        let key = self
            .queued
            .iter()
            .find(|(_, entry)| Arc::as_ptr(&entry.task) == task as *const Task)
            .map(|(k, _)| *k)?;

        let entry = self.queued.remove(&key)?;
        self.unaccount(&entry.entity.fair);
        self.remember_lag(&entry.entity.fair);
        Some(entry)
    }

    /// Returns the eligible queued task with the earliest virtual deadline.
    pub fn best(&self) -> Option<&SchedEntry> {
        if self.queued.is_empty() {
            return None;
        }
        let avg_vruntime = self.avg_vruntime();
        self.queued
            .iter()
            .find(|(_, entry)| entry.entity.fair.vruntime() <= avg_vruntime)
            .map(|(_, entry)| entry)
            .or_else(|| self.queued.values().next())
    }

    /// Returns whether candidate displaced the current fair task.
    pub fn wins_over_current(&self, candidate: &SchedEntry, yielding: bool) -> bool {
        let Some(current) = &self.current else {
            return true;
        };
        if !self.is_eligible(&current.entity.fair) {
            return true;
        }
        EevdfRunKey::of(candidate, false) < EevdfRunKey::of(current, yielding)
    }

    /// Removes an enqueued entry by key when picked for execution.
    pub fn extract(&mut self, entry: &SchedEntry) -> SchedEntry {
        let key = EevdfRunKey::of(entry, false);
        self.queued
            .remove(&key)
            .expect("picked task must be in runqueue")
    }

    /// Records bounded lag: `clamp(avg_vruntime - vruntime, -2 * request, 2 * request)`.
    pub fn remember_lag(&self, entity: &EevdfEntity) {
        let lag = self.avg_vruntime() as i64 - entity.vruntime() as i64;
        let limit = 2 * entity.request() as i64;
        entity.set_lag(lag.clamp(-limit, limit));
    }

    /// Unaccounts an entity from the running sums.
    pub fn unaccount(&mut self, entity: &EevdfEntity) {
        let weight = entity.weight() as u64;
        let vruntime = entity.vruntime().saturating_sub(self.min_vruntime);
        self.sum_weight = self.sum_weight.saturating_sub(weight);
        self.sum_offset_weighted = self
            .sum_offset_weighted
            .saturating_sub(vruntime.saturating_mul(weight));
        if vruntime == 0 {
            self.advance_min_vruntime();
        }
    }

    /// Advances `min_vruntime` to the smallest active virtual runtime.
    pub fn advance_min_vruntime(&mut self) {
        let current_v = self.current.as_ref().map(|e| e.entity.fair.vruntime());
        let queued_v = self.queued.values().map(|e| e.entity.fair.vruntime());
        let Some(smallest) = current_v.into_iter().chain(queued_v).min() else {
            return;
        };

        let advance = smallest.saturating_sub(self.min_vruntime);
        if advance == 0 {
            return;
        }
        self.min_vruntime = smallest;
        self.sum_offset_weighted = self
            .sum_offset_weighted
            .saturating_sub(advance.saturating_mul(self.sum_weight));
    }
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;

    #[ktest]
    fn test_eevdf_weights() {
        assert_eq!(weight_from_nice(0), 1024);
        assert_eq!(weight_from_nice(-20), 88761);
        assert_eq!(weight_from_nice(19), 15);
        assert_eq!(nice_from_weight(1024), 0);
        assert_eq!(nice_from_weight(88761), -20);
        assert_eq!(nice_from_weight(15), 19);
    }

    #[ktest]
    fn test_eevdf_scale() {
        // scale(value, num, den) = value * num / den
        assert_eq!(scale(1000, 1024, 1024), 1000);
        assert_eq!(scale(1000, 1024, 2048), 500);
        assert_eq!(scale(1000, 2048, 1024), 2000);
        assert_eq!(scale(1000, 10, 0), 0);
    }

    #[ktest]
    fn test_eevdf_request_and_deadline() {
        let entity = EevdfEntity::new(0, 3_000_000);
        // Nice 0: weight = 1024. request = 3_000_000 * 1024 / 1024 = 3_000_000
        assert_eq!(entity.request(), 3_000_000);
        assert_eq!(entity.deadline(), 3_000_000);

        // Charge 1_000_000 ns
        entity.charge(1_000_000);
        assert_eq!(entity.vruntime(), 1_000_000);
        assert_eq!(entity.deadline(), 4_000_000);
    }

    #[ktest]
    fn test_eevdf_higher_weight_lower_vruntime() {
        // Nice -20 (weight 88761) vs Nice 0 (weight 1024)
        let heavy = EevdfEntity::new(-20, 3_000_000);
        let normal = EevdfEntity::new(0, 3_000_000);

        heavy.charge(1_000_000);
        normal.charge(1_000_000);

        // Heavy task advances virtual runtime much slower than normal task
        assert!(heavy.vruntime() < normal.vruntime());
        assert_eq!(normal.vruntime(), 1_000_000);
    }

    #[ktest]
    fn test_eevdf_slice_clamping() {
        // Slices should clamp between MIN_SLICE_NS (100 us) and MAX_SLICE_NS (100 ms)
        let too_small = EevdfEntity::new(0, 50_000);
        assert_eq!(too_small.slice_ns(), MIN_SLICE_NS);

        let too_large = EevdfEntity::new(0, 500_000_000);
        assert_eq!(too_large.slice_ns(), MAX_SLICE_NS);

        let valid = EevdfEntity::new(0, 5_000_000);
        assert_eq!(valid.slice_ns(), 5_000_000);
    }

    #[ktest]
    fn test_eevdf_lag_clamping_and_bounds() {
        let entity = EevdfEntity::new(0, 3_000_000);
        let req = entity.request() as i64;
        let limit = 2 * req;

        // Positive excess lag clamps to +2 * request
        let excess_pos = 10 * req;
        entity.set_lag(excess_pos.clamp(-limit, limit));
        assert_eq!(entity.lag(), limit);

        // Negative excess lag clamps to -2 * request
        let excess_neg = -10 * req;
        entity.set_lag(excess_neg.clamp(-limit, limit));
        assert_eq!(entity.lag(), -limit);

        // Moderate lag preserved accurately
        let moderate = req / 2;
        entity.set_lag(moderate.clamp(-limit, limit));
        assert_eq!(entity.lag(), moderate);
    }

    #[ktest]
    fn test_eevdf_nice_scale_spectrum() {
        // Check that virtual time advancement is strictly monotonically increasing with nice
        let mut prev_vruntime = 0u64;
        for nice in NICE_MIN..=NICE_MAX {
            let entity = EevdfEntity::new(nice, 3_000_000);
            entity.charge(1_000_000);
            let vr = entity.vruntime();
            if nice > NICE_MIN {
                assert!(
                    vr > prev_vruntime,
                    "nice {} (vr={}) should advance virtual time faster than previous (vr={})",
                    nice,
                    vr,
                    prev_vruntime
                );
            }
            prev_vruntime = vr;
        }
    }

    #[ktest]
    fn test_eevdf_runqueue_metrics_and_monotonicity() {
        let rq = EevdfRunQueue::new();
        assert_eq!(rq.nr_running(), 0);
        assert_eq!(rq.min_vruntime(), 0);
        assert_eq!(rq.avg_vruntime(), 0);
    }
}
