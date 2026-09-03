//! Kernel Preemption Control.
//!
//! Provides per-CPU preemption counters to guard kernel critical sections.
//! When `preempt_count > 0`, the scheduler must not preempt the currently executing
//! kernel thread.

use core::sync::atomic::{AtomicU32, Ordering};

/// Maximum number of CPUs supported for preemption tracking.
pub const MAX_PREEMPT_CPUS: usize = 256;

/// Per-CPU preemption counters.
static PREEMPT_COUNTERS: [AtomicU32; MAX_PREEMPT_CPUS] =
    [const { AtomicU32::new(0) }; MAX_PREEMPT_CPUS];

/// Disable preemption on the calling CPU core.
#[inline(always)]
pub fn preempt_disable() {
    let cpu_id = crate::arch::cpu_id() as usize;
    if cpu_id < MAX_PREEMPT_CPUS {
        PREEMPT_COUNTERS[cpu_id].fetch_add(1, Ordering::Relaxed);
    }
}

/// Enable preemption on the calling CPU core.
#[inline(always)]
pub fn preempt_enable() {
    let cpu_id = crate::arch::cpu_id() as usize;
    if cpu_id < MAX_PREEMPT_CPUS {
        let prev = PREEMPT_COUNTERS[cpu_id].fetch_sub(1, Ordering::Release);
        debug_assert!(prev > 0, "Preemption counter underflow on CPU {}", cpu_id);
    }
}

/// Returns `true` if preemption is currently allowed on the calling CPU core.
#[inline(always)]
pub fn can_preempt() -> bool {
    let cpu_id = crate::arch::cpu_id() as usize;
    if cpu_id < MAX_PREEMPT_CPUS {
        PREEMPT_COUNTERS[cpu_id].load(Ordering::Acquire) == 0
    } else {
        false
    }
}

/// Returns the current preemption count on the calling CPU core.
#[inline(always)]
pub fn preempt_count() -> u32 {
    let cpu_id = crate::arch::cpu_id() as usize;
    if cpu_id < MAX_PREEMPT_CPUS {
        PREEMPT_COUNTERS[cpu_id].load(Ordering::Acquire)
    } else {
        1
    }
}

/// RAII guard that disables preemption for its lexical scope.
pub struct PreemptGuard {
    _private: (),
}

impl PreemptGuard {
    pub fn new() -> Self {
        preempt_disable();
        Self { _private: () }
    }
}

impl Default for PreemptGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for PreemptGuard {
    fn drop(&mut self) {
        preempt_enable();
    }
}
