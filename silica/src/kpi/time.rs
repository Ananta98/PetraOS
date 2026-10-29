// SPDX-License-Identifier: GPL-2.0

//! One-shot Linux-style timers (`timer_list`): a wheel of deadlines that
//! expire on timer ticks, complete a [`KCompletion`], and optionally run a
//! small callback in interrupt context.
//!
//! Callbacks run in the timer interrupt on the BSP, mirroring Linux soft
//! timer semantics: they must not sleep or take blocking locks; longer work
//! should hand off to `schedule_work`.
//!
//! The deadline granularity is one timer tick (1 ms at OSTD's
//! [`ostd::timer::TIMER_FREQ`] of 1000 Hz), rounded up.

use alloc::{boxed::Box, collections::BTreeMap, sync::Arc, vec::Vec};
use core::time::Duration;
use ostd::cpu::{CpuId, PinCurrentCpu};
use ostd::timer::Jiffies;
use ostd::{irq, sync::{LocalIrqDisabled, SpinLock}};

use super::sync::KCompletion;

/// A boxed timer callback executed in interrupt context on expiry.
pub type TimerCallback = Box<dyn Fn() + Send + Sync>;

struct Entry {
    callback: Option<TimerCallback>,
    expired: KCompletion,
}

/// The timer wheel, keyed by deadline in jiffies. Entries with the same
/// deadline expire together, one bucket scan per tick. Timers never migrate
/// buckets after insertion.
static EXPIRIES: SpinLock<BTreeMap<u64, Vec<Arc<Entry>>>, LocalIrqDisabled> =
    SpinLock::new(BTreeMap::new());

/// An armed one-shot timer.
#[derive(Clone)]
pub struct KTimer(Arc<Entry>);

/// Arms a one-shot timer expiring `after` from now.
///
/// Expiry completes the timer: [`KTimer::wait`] unblocks and, if given, the
/// callback runs in interrupt context — keep it brief and non-sleeping.
pub fn arm(after: Duration, callback: Option<TimerCallback>) -> KTimer {
    let deadline = Jiffies::elapsed().as_u64() + jiffies_for(after);
    let entry = Arc::new(Entry {
        callback,
        expired: KCompletion::new(),
    });
    EXPIRIES
        .lock()
        .entry(deadline)
        .or_default()
        .push(Arc::clone(&entry));
    KTimer(entry)
}

impl KTimer {
    /// Sleeps until the timer expires (depending on completion counts it can
    /// also return promptly if a prior wait has not consumed the expiry).
    pub fn wait(&self) {
        self.0.expired.wait();
    }

    /// Consumes the expiry without sleeping; false if not yet expired.
    pub fn try_wait(&self) -> bool {
        self.0.expired.try_wait()
    }

    /// Returns the timer's completion, for combining with other waits.
    pub fn completion(&self) -> &KCompletion {
        &self.0.expired
    }

    /// Removes the timer from the wheel if not yet expired, returning whether
    /// it was still pending. Expiry already in progress cannot be cancelled.
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

/// Returns the current elapsed timer ticks (jiffies).
pub fn jiffies() -> u64 {
    Jiffies::elapsed().as_u64()
}

/// Converts a duration to tick counts, rounding up so a timer never fires
/// before its deadline.
fn jiffies_for(after: Duration) -> u64 {
    let nanos = after.as_nanos() as u64;
    ((nanos * ostd::timer::TIMER_FREQ + 999_999_999) / 1_000_000_000).max(1)
}

/// Expires due timers once per tick, on the BSP where jiffies advance.
fn process_expired() {
    // Expiry runs on one CPU by design, like Linux's soft timer base: expiry
    // work is serialized and the wheel has a single writer, which keeps it
    // simple enough to reason about soundness.
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
        drop(map);
        due
    };

    for entry in due {
        if let Some(callback) = &entry.callback {
            callback();
        }
        entry.expired.complete();
    }
}

/// Hooks expiry processing into the periodic timer interrupt.
pub(super) fn init() {
    ostd::timer::register_callback_on_cpu(process_expired);
}
