// SPDX-License-Identifier: GPL-2.0

//! System load average computation.
//!
//! Tracks exponentially decayed 1, 5, and 15-minute load averages using integer
//! fixed-point arithmetic, mirroring the decay factors used in Linux `/proc/loadavg`.

use core::sync::atomic::{AtomicU64, Ordering};
use ostd::timer::{Jiffies, TIMER_FREQ};

/// Interval between load average decay calculations (seconds).
const LOAD_INTERVAL_S: u64 = 5;

/// Number of timer ticks per decay interval.
const TICKS_PER_INTERVAL: u64 = LOAD_INTERVAL_S * TIMER_FREQ;

/// Base scale for load average integer representation.
pub const LOAD_AVG_MAX: u64 = 65535;

/// Fractional bits used for fixed-point precision.
const LOAD_AVG_BITS: u32 = 8;

/// Effective fixed-point scale factor (1.0 load = LOAD_AVG_SCALE).
pub const LOAD_AVG_SCALE: u64 = LOAD_AVG_MAX << LOAD_AVG_BITS;

/// Fixed-point denominator for exponential decay factors.
const LOAD_FACTOR: u64 = 2047;

/// 1-minute decay constant: e^(-5s / 1min) * LOAD_FACTOR.
const EXP_1: u64 = 1884;

/// 5-minute decay constant: e^(-5s / 5min) * LOAD_FACTOR.
const EXP_5: u64 = 2014;

/// 15-minute decay constant: e^(-5s / 15min) * LOAD_FACTOR.
const EXP_15: u64 = 2037;

static INTERVAL_SUM: AtomicU64 = AtomicU64::new(0);
static INTERVAL_TICKS: AtomicU64 = AtomicU64::new(0);
static INTERVAL_END: AtomicU64 = AtomicU64::new(TICKS_PER_INTERVAL);

static AVENRUN: [AtomicU64; 3] = [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)];

/// Three exponentially decayed system load averages.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LoadAvg {
    /// 1-minute load average in fixed-point units.
    pub one: u64,
    /// 5-minute load average in fixed-point units.
    pub five: u64,
    /// 15-minute load average in fixed-point units.
    pub fifteen: u64,
}

impl LoadAvg {
    /// Returns the 1-minute load average as an f64.
    pub fn one_f64(&self) -> f64 {
        self.one as f64 / LOAD_AVG_SCALE as f64
    }

    /// Returns the 5-minute load average as an f64.
    pub fn five_f64(&self) -> f64 {
        self.five as f64 / LOAD_AVG_SCALE as f64
    }

    /// Returns the 15-minute load average as an f64.
    pub fn fifteen_f64(&self) -> f64 {
        self.fifteen as f64 / LOAD_AVG_SCALE as f64
    }
}

impl core::fmt::Display for LoadAvg {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for (index, value) in [self.one, self.five, self.fifteen].iter().enumerate() {
            if index > 0 {
                f.write_str(" ")?;
            }
            let thousandths = (((*value as u128) * 1000 + (LOAD_AVG_SCALE as u128) / 2)
                / (LOAD_AVG_SCALE as u128)) as u64;
            write!(f, "{}.{:02}", thousandths / 1000, (thousandths % 1000) / 10)?;
        }
        Ok(())
    }
}

/// Returns the current system load average.
pub fn loadavg() -> LoadAvg {
    LoadAvg {
        one: AVENRUN[0].load(Ordering::Relaxed),
        five: AVENRUN[1].load(Ordering::Relaxed),
        fifteen: AVENRUN[2].load(Ordering::Relaxed),
    }
}

/// Accounts for one CPU timer tick with `nr_running` runnable tasks.
pub fn on_tick(nr_running: usize) {
    let tick = Jiffies::elapsed().as_u64();

    INTERVAL_SUM.fetch_add(nr_running as u64, Ordering::Relaxed);
    INTERVAL_TICKS.fetch_add(1, Ordering::Relaxed);

    let end = INTERVAL_END.load(Ordering::Relaxed);
    if tick < end {
        return;
    }
    if INTERVAL_END
        .compare_exchange(
            end,
            next_boundary(end, tick),
            Ordering::Relaxed,
            Ordering::Relaxed,
        )
        .is_err()
    {
        return;
    }

    let sum = INTERVAL_SUM.swap(0, Ordering::Relaxed);
    let ticks = INTERVAL_TICKS.swap(0, Ordering::Relaxed);
    if ticks == 0 {
        return;
    }

    store(decay(
        [
            AVENRUN[0].load(Ordering::Relaxed),
            AVENRUN[1].load(Ordering::Relaxed),
            AVENRUN[2].load(Ordering::Relaxed),
        ],
        scale(sum, LOAD_AVG_SCALE, ticks),
    ));
}

fn store(avenrun: [u64; 3]) {
    for (target, value) in AVENRUN.iter().zip(avenrun) {
        target.store(value, Ordering::Relaxed);
    }
}

fn next_boundary(end: u64, tick: u64) -> u64 {
    let intervals = tick.saturating_sub(end) / TICKS_PER_INTERVAL + 1;
    end + intervals * TICKS_PER_INTERVAL
}

fn decay(avenrun: [u64; 3], sample: u64) -> [u64; 3] {
    let mut decayed = [0; 3];
    for (index, out) in decayed.iter_mut().enumerate() {
        let old = avenrun[index];
        let exp = [EXP_1, EXP_5, EXP_15][index];
        let difference = sample as i64 - old as i64;
        let step = scale(difference.unsigned_abs(), LOAD_FACTOR - exp, LOAD_FACTOR) as i64;
        let step = if difference < 0 { -step } else { step };
        *out = (old as i64 + step).max(0) as u64;
    }
    decayed
}

fn scale(value: u64, numerator: u64, denominator: u64) -> u64 {
    let scaled = (value as u128) * (numerator as u128) / (denominator as u128);
    u64::try_from(scaled).unwrap_or(u64::MAX)
}
