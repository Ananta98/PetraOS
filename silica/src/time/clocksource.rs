// SPDX-License-Identifier: GPL-2.0

//! Unix Clocksource Subsystem and Monotonic Cycle Counter Abstraction.
//!
//! Mirrors the Linux `struct clocksource` model. Provides fixed-point cycle-to-nanosecond
//! scaling, rating-based dynamic selection, and cycle rollover tracking.

use alloc::{sync::Arc, vec::Vec};
use bitflags::bitflags;
use ostd::sync::{LocalIrqDisabled, SpinLock};
use spin::Once;

use crate::api::errno::Result;

// ----------------------------------------------------------------------------
// Clocksource Flags
// ----------------------------------------------------------------------------

bitflags! {
    /// Characteristics and capabilities of a clocksource.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct ClocksourceFlags: u32 {
        /// Clocksource continues running in idle/low-power states.
        const CONTINUOUS     = 1 << 0;
        /// Clocksource requires watchdog verification.
        const MUST_VERIFY    = 1 << 1;
        /// Clocksource can act as a reference watchdog clock.
        const WATCHDOG       = 1 << 2;
        /// Clocksource has high resolution (< 1 microsecond) and low latency.
        const VALID_FOR_HRES = 1 << 3;
        /// Clocksource has been flagged as unstable or unsynchronized across CPUs.
        const UNSTABLE       = 1 << 4;
    }
}

// ----------------------------------------------------------------------------
// Clocksource Trait
// ----------------------------------------------------------------------------

/// Trait implemented by hardware and virtual cycle counter devices.
pub trait Clocksource: Send + Sync {
    /// Human-readable name of the clocksource (e.g. "tsc", "jiffies").
    fn name(&self) -> &'static str;

    /// Quality and selection rating (higher value = preferred).
    ///
    /// Typical ratings:
    /// - Jiffies: 50
    /// - PIT: 110
    /// - HPET: 250
    /// - TSC: 300
    fn rating(&self) -> u32;

    /// Reads the current monotonic hardware cycle count.
    fn read(&self) -> u64;

    /// Cycle counter bitmask (e.g. `!0u64` for 64-bit, `0xFFFF_FFFF` for 32-bit).
    fn mask(&self) -> u64;

    /// Multiplier for fixed-point nanosecond conversion: `(cycles * mult) >> shift`.
    fn mult(&self) -> u32;

    /// Shift factor for fixed-point nanosecond conversion.
    fn shift(&self) -> u32;

    /// Capability flags.
    fn flags(&self) -> ClocksourceFlags;

    /// Optional initialization or power-up hook.
    fn enable(&self) -> Result<()> {
        Ok(())
    }

    /// Optional tear-down hook when deselecting.
    fn disable(&self) {}
}

// ----------------------------------------------------------------------------
// Fixed-Point Scaling Math (Linux mult/shift algorithm)
// ----------------------------------------------------------------------------

/// Calculates optimal fixed-point `mult` and `shift` values for converting
/// cycle counts at `freq_hz` to nanoseconds.
///
/// Formula: `(cycles * mult) >> shift ≈ cycles * (1_000_000_000 / freq_hz)`.
pub fn calc_mult_shift(freq_hz: u64) -> (u32, u32) {
    if freq_hz == 0 {
        return (1, 0);
    }

    // Dynamically find the maximum shift (up to 31) where mult fits in u32.
    // This mirrors Linux's `clocks_calc_mult_shift` algorithm, providing maximum
    // fractional precision for any frequency without 32-bit truncation.
    let mut shift = 31;
    while shift > 0 {
        let mult = ((1_000_000_000u128 << shift) + (freq_hz as u128 / 2)) / freq_hz as u128;
        if mult <= u32::MAX as u128 {
            return (mult as u32, shift);
        }
        shift -= 1;
    }

    let mult = (1_000_000_000u128 + (freq_hz as u128 / 2)) / freq_hz as u128;
    (mult as u32, 0)
}

// ----------------------------------------------------------------------------
// Clocksource Manager
// ----------------------------------------------------------------------------

/// Manages registered clocksources, active selection, and monotonic accumulation.
pub struct ClocksourceManager {
    sources: Vec<Arc<dyn Clocksource>>,
    active: Option<Arc<dyn Clocksource>>,
    cycle_last: u64,
    base_monotonic_ns: u64,
}

impl ClocksourceManager {
    pub const fn new() -> Self {
        Self {
            sources: Vec::new(),
            active: None,
            cycle_last: 0,
            base_monotonic_ns: 0,
        }
    }

    /// Accumulates cycles from the active clocksource into `base_monotonic_ns`.
    pub fn accumulate(&mut self) {
        if let Some(cs) = &self.active {
            let now = cs.read();
            let delta = now.wrapping_sub(self.cycle_last) & cs.mask();
            let delta_ns = ((delta as u128) * (cs.mult() as u128)) >> cs.shift();
            self.base_monotonic_ns = self.base_monotonic_ns.saturating_add(delta_ns as u64);
            self.cycle_last = now;
        }
    }

    /// Returns the current monotonic time in nanoseconds since boot.
    pub fn read_monotonic_ns(&self) -> u64 {
        if let Some(cs) = &self.active {
            let now = cs.read();
            let delta = now.wrapping_sub(self.cycle_last) & cs.mask();
            let delta_ns = ((delta as u128) * (cs.mult() as u128)) >> cs.shift();
            self.base_monotonic_ns.saturating_add(delta_ns as u64)
        } else {
            // Fallback to Jiffies
            ostd::timer::Jiffies::elapsed().as_duration().as_nanos() as u64
        }
    }

    /// Registers a new clocksource and selects it if its rating is higher than
    /// the currently active clocksource.
    pub fn register(&mut self, cs: Arc<dyn Clocksource>) -> Result<()> {
        ostd::info!(
            "Clocksource: Registering '{}' (rating {}, mask 0x{:X})",
            cs.name(),
            cs.rating(),
            cs.mask()
        );

        let should_switch = match &self.active {
            None => true,
            Some(active) => cs.rating() > active.rating(),
        };

        if should_switch {
            self.switch_to(cs.clone())?;
        }

        self.sources.push(cs);
        Ok(())
    }

    /// Switches the active clocksource seamlessly while preserving monotonic continuity.
    fn switch_to(&mut self, new_cs: Arc<dyn Clocksource>) -> Result<()> {
        // 1. Bring accumulated monotonic time up to date with current clocksource
        self.accumulate();

        // 2. Enable new clocksource
        new_cs.enable()?;

        // 3. Disable previous clocksource if present
        if let Some(old_cs) = &self.active {
            old_cs.disable();
        }

        // 4. Latch initial cycle count for new clocksource
        self.cycle_last = new_cs.read();
        self.active = Some(new_cs.clone());

        ostd::info!(
            "Clocksource: Switched active clocksource to '{}' (rating {})",
            new_cs.name(),
            new_cs.rating()
        );

        Ok(())
    }

    /// Returns the name of the currently active clocksource.
    pub fn active_name(&self) -> &'static str {
        self.active.as_ref().map(|cs| cs.name()).unwrap_or("none")
    }

    /// Returns whether a high-resolution clocksource is active.
    pub fn is_high_res(&self) -> bool {
        self.active
            .as_ref()
            .map(|cs| cs.flags().contains(ClocksourceFlags::VALID_FOR_HRES))
            .unwrap_or(false)
    }
}

// ----------------------------------------------------------------------------
// Global Manager Instance & Public API
// ----------------------------------------------------------------------------

static CS_MANAGER: Once<SpinLock<ClocksourceManager, LocalIrqDisabled>> = Once::new();

fn manager() -> &'static SpinLock<ClocksourceManager, LocalIrqDisabled> {
    CS_MANAGER.call_once(|| SpinLock::new(ClocksourceManager::new()))
}

/// Registers a new hardware clocksource.
pub fn register_clocksource(cs: Arc<dyn Clocksource>) -> Result<()> {
    manager().lock().register(cs)
}

/// Reads current monotonic time in nanoseconds from the active clocksource.
pub fn monotonic_now_ns() -> u64 {
    manager().lock().read_monotonic_ns()
}

/// Accumulates elapsed cycles to advance the base monotonic time.
pub fn accumulate_cycles() {
    manager().lock().accumulate();
}

/// Returns the name of the active clocksource.
pub fn active_clocksource_name() -> &'static str {
    manager().lock().active_name()
}

/// Returns whether the active clocksource supports high-resolution timing.
pub fn is_high_resolution() -> bool {
    manager().lock().is_high_res()
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;

    #[ktest]
    fn calc_mult_shift_precision() {
        // 1 GHz: exactly 1 cycle = 1 ns
        let (mult_1g, shift_1g) = calc_mult_shift(1_000_000_000);
        let ns_1g = ((1_000_000_000u128 * mult_1g as u128) >> shift_1g) as u64;
        assert_eq!(ns_1g, 1_000_000_000);

        // 2.4 GHz: 2.4 * 10^9 cycles should yield 1_000_000_000 ns within 1 ns error margin
        let (mult_24g, shift_24g) = calc_mult_shift(2_400_000_000);
        let ns_24g = ((2_400_000_000u128 * mult_24g as u128) >> shift_24g) as u64;
        let diff = (ns_24g as i64 - 1_000_000_000i64).abs();
        assert!(diff <= 1, "Diff {diff} exceeds 1ns for 2.4GHz mult/shift scaling");
    }
}

