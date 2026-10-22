// SPDX-License-Identifier: GPL-2.0

//! x86_64 Time Stamp Counter (TSC) and Jiffies Clocksource Implementation.
//!
//! Provides the primary hardware high-resolution `tsc` clocksource (rating 300)
//! and the standard fallback `jiffies` clocksource (rating 50).

use alloc::sync::Arc;
use ostd::timer::{Jiffies, TIMER_FREQ};

use super::clocksource::{Clocksource, ClocksourceFlags, calc_mult_shift, register_clocksource};
use crate::uapi::errno::Result;

// ----------------------------------------------------------------------------
// TSC Clocksource
// ----------------------------------------------------------------------------

/// High-resolution x86_64 Time Stamp Counter clocksource.
pub struct TscClocksource {
    freq_hz: u64,
    mult: u32,
    shift: u32,
}

impl TscClocksource {
    /// Creates a new TSC clocksource with the specified frequency in Hz.
    pub fn new(freq_hz: u64) -> Self {
        let (mult, shift) = calc_mult_shift(freq_hz);
        Self {
            freq_hz,
            mult,
            shift,
        }
    }

    /// Returns the detected or calibrated frequency of this TSC in Hz.
    pub fn freq_hz(&self) -> u64 {
        self.freq_hz
    }
}

impl Clocksource for TscClocksource {
    fn name(&self) -> &'static str {
        "tsc"
    }

    fn rating(&self) -> u32 {
        300
    }

    fn read(&self) -> u64 {
        ostd::arch::read_tsc()
    }

    fn mask(&self) -> u64 {
        !0u64
    }

    fn mult(&self) -> u32 {
        self.mult
    }

    fn shift(&self) -> u32 {
        self.shift
    }

    fn flags(&self) -> ClocksourceFlags {
        ClocksourceFlags::CONTINUOUS | ClocksourceFlags::VALID_FOR_HRES
    }
}

// ----------------------------------------------------------------------------
// Jiffies Clocksource (Fallback)
// ----------------------------------------------------------------------------

/// Timer interrupt tick (Jiffies) fallback clocksource.
pub struct JiffiesClocksource {
    mult: u32,
    shift: u32,
}

impl JiffiesClocksource {
    pub fn new() -> Self {
        let (mult, shift) = calc_mult_shift(TIMER_FREQ);
        Self { mult, shift }
    }
}

impl Default for JiffiesClocksource {
    fn default() -> Self {
        Self::new()
    }
}

impl Clocksource for JiffiesClocksource {
    fn name(&self) -> &'static str {
        "jiffies"
    }

    fn rating(&self) -> u32 {
        50
    }

    fn read(&self) -> u64 {
        Jiffies::elapsed().as_u64()
    }

    fn mask(&self) -> u64 {
        !0u64
    }

    fn mult(&self) -> u32 {
        self.mult
    }

    fn shift(&self) -> u32 {
        self.shift
    }

    fn flags(&self) -> ClocksourceFlags {
        ClocksourceFlags::CONTINUOUS
    }
}

// ----------------------------------------------------------------------------
// Calibration and Initialization
// ----------------------------------------------------------------------------

/// Calibrates the TSC frequency against system timer jiffies if hardware
/// enumeration was not available.
fn calibrate_tsc_via_jiffies() -> u64 {
    let start_jiffies = Jiffies::elapsed().as_u64();
    let start_tsc = ostd::arch::read_tsc();

    // Sample across 10 jiffies (10 ms at 1000 Hz)
    const TARGET_TICKS: u64 = 10;
    while Jiffies::elapsed().as_u64().saturating_sub(start_jiffies) < TARGET_TICKS {
        core::hint::spin_loop();
    }

    let elapsed_jiffies = Jiffies::elapsed().as_u64().saturating_sub(start_jiffies);
    let elapsed_tsc = ostd::arch::read_tsc().saturating_sub(start_tsc);

    if elapsed_jiffies == 0 {
        return 0;
    }

    (elapsed_tsc * TIMER_FREQ) / elapsed_jiffies
}

/// Initializes and registers clocksources.
pub fn init() -> Result<()> {
    // 1. Register fallback jiffies clocksource
    let jiffies_cs = Arc::new(JiffiesClocksource::new());
    register_clocksource(jiffies_cs)?;

    // 2. Determine TSC frequency
    let mut tsc_freq = ostd::arch::tsc_freq();
    if tsc_freq == 0 {
        tsc_freq = calibrate_tsc_via_jiffies();
    }

    // 3. Register TSC clocksource if valid
    if tsc_freq > 1_000_000 {
        ostd::info!(
            "TSC: Initialized TSC clocksource at {} MHz ({} Hz)",
            tsc_freq / 1_000_000,
            tsc_freq
        );
        let tsc_cs = Arc::new(TscClocksource::new(tsc_freq));
        register_clocksource(tsc_cs)?;
    } else {
        ostd::warn!("TSC: Could not determine valid TSC frequency; using jiffies fallback");
    }

    Ok(())
}
