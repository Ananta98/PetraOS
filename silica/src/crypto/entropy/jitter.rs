// SPDX-License-Identifier: GPL-2.0

//! Timing, clock cycle, and scheduler jitter entropy collector.

/// Collects CPU cycle counter and high-resolution timer jitter.
pub fn get_jitter_entropy_u64() -> u64 {
    let elapsed = ostd::timer::Jiffies::elapsed().as_duration();
    let nanos = elapsed.as_nanos() as u64;
    let sched_ticks = crate::sched::now();
    let tsc = ostd::arch::read_tsc();

    // Stir timer, scheduler state, and TSC
    nanos ^ sched_ticks.rotate_left(19) ^ tsc.rotate_left(37)
}
