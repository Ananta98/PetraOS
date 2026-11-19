// SPDX-License-Identifier: GPL-2.0

//! Kernel soft timer wheel (`KernelTimer`): one-shot deadlines that expire on
//! periodic timer ticks, complete a [`Completion`], and optionally invoke a
//! short non-blocking callback in interrupt context.
//!
//! Callbacks execute in the BSP timer interrupt handler; they must not sleep
//! or take blocking locks. Longer work should be deferred to
//! [`crate::proc::workqueue`].
//!
//! Deadline resolution is one timer tick (1 ms at OSTD's
//! [`ostd::timer::TIMER_FREQ`] of 1000 Hz), rounded up.

use alloc::{boxed::Box, collections::BTreeMap, sync::Arc, vec::Vec};
use core::time::Duration;
use ostd::cpu::{CpuId, PinCurrentCpu};
use ostd::irq;
use ostd::sync::{LocalIrqDisabled, SpinLock};
use ostd::timer::Jiffies;

use crate::sync::Completion;

/// A boxed timer callback executed in interrupt context on expiry.
pub type TimerCallback = Box<dyn Fn() + Send + Sync>;

struct Entry {
    callback: Option<TimerCallback>,
    expired: Completion,
}

/// Timer wheel keyed by deadline in jiffies. Entries sharing a deadline expire
/// together on the BSP timer tick.
static EXPIRIES: SpinLock<BTreeMap<u64, Vec<Arc<Entry>>>, LocalIrqDisabled> =
    SpinLock::new(BTreeMap::new());

/// An armed one-shot kernel timer.
#[derive(Clone)]
pub struct KernelTimer(Arc<Entry>);

/// Arms a one-shot timer expiring `after` from now.
///
/// Expiry completes the timer: [`KernelTimer::wait`] unblocks and, if given,
/// the callback runs in interrupt context — keep it brief and non-sleeping.
pub fn arm(after: Duration, callback: Option<TimerCallback>) -> KernelTimer {
    let deadline = Jiffies::elapsed().as_u64() + jiffies_for(after);
    let entry = Arc::new(Entry {
        callback,
        expired: Completion::new(),
    });
    EXPIRIES
        .lock()
        .entry(deadline)
        .or_default()
        .push(Arc::clone(&entry));
    KernelTimer(entry)
}

impl KernelTimer {
    /// Sleeps until the timer expires.
    pub fn wait(&self) {
        self.0.expired.wait();
    }

    /// Consumes the expiry without sleeping; returns `false` if still pending.
    pub fn try_wait(&self) -> bool {
        self.0.expired.try_wait()
    }

    /// Returns the underlying completion for combining with other waits.
    pub fn completion(&self) -> &Completion {
        &self.0.expired
    }

    /// Cancels the timer if it has not yet expired, returning whether it was
    /// removed while still pending.
    pub fn cancel(&self) -> bool {
        let mut map = EXPIRIES.lock();
        for entries in map.values_mut() {
            let before = entries.len();
            entries.retain(|entry| !Arc::ptr_eq(entry, &self.0));
            if entries.len() != before {
                return true;
            }
        }
        false
    }
}

/// Returns the elapsed timer ticks (jiffies) since boot.
pub fn jiffies() -> u64 {
    Jiffies::elapsed().as_u64()
}

/// Sleeps for at least `ms` milliseconds.
pub fn msleep(ms: u64) {
    if ms == 0 {
        return;
    }
    arm(Duration::from_millis(ms), None).wait();
}

/// Sleeps for a duration between `min_us` and `max_us` microseconds.
pub fn usleep_range(min_us: u64, max_us: u64) {
    let us = if max_us > min_us {
        min_us + (max_us - min_us) / 2
    } else {
        min_us
    };
    if us == 0 {
        return;
    }
    arm(Duration::from_micros(us), None).wait();
}

/// Short busy-wait delay in microseconds.
pub fn udelay(us: u64) {
    let start = jiffies();
    let ticks = jiffies_for(Duration::from_micros(us));
    while jiffies().saturating_sub(start) < ticks {
        core::hint::spin_loop();
    }
}

/// Short busy-wait delay in milliseconds.
pub fn mdelay(ms: u64) {
    udelay(ms.saturating_mul(1000));
}

/// Returns monotonic elapsed time in nanoseconds derived from jiffies.
pub fn ktime_get_ns() -> u64 {
    (jiffies().saturating_mul(1_000_000_000)) / ostd::timer::TIMER_FREQ
}

/// Converts a duration to tick counts, rounding up so a timer never fires early.
fn jiffies_for(after: Duration) -> u64 {
    let nanos = after.as_nanos() as u64;
    ((nanos * ostd::timer::TIMER_FREQ + 999_999_999) / 1_000_000_000).max(1)
}

/// Expires due timers once per tick on the BSP where jiffies advance.
fn process_expired() {
    let irq_guard = irq::disable_local();
    if irq_guard.current_cpu() != CpuId::bsp() {
        return;
    }

    let now = Jiffies::elapsed().as_u64();
    let due: Vec<Arc<Entry>> = {
        let mut map = EXPIRIES.lock();
        let mut due = Vec::new();
        while let Some(deadline) = map.keys().next().copied() {
            if deadline > now {
                break;
            }
            due.extend(map.remove(&deadline).expect("key just observed"));
        }
        due
    };

    for entry in due {
        if let Some(callback) = &entry.callback {
            callback();
        }
        entry.expired.complete();
    }
}

/// Registers soft timer expiry processing on the periodic timer interrupt.
pub(crate) fn init() {
    ostd::timer::register_callback_on_cpu(process_expired);
}
