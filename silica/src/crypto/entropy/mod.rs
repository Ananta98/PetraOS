// SPDX-License-Identifier: GPL-2.0

//! Multi-source hardware & kernel entropy harvester.

pub mod jitter;
pub mod rdrand;

pub use jitter::get_jitter_entropy_u64;
pub use rdrand::get_hardware_random_u64;

/// Harvests combined entropy from hardware RDRAND (if available) and CPU/scheduler jitter.
pub fn harvest_entropy_u64() -> u64 {
    let jitter = get_jitter_entropy_u64();
    if let Some(hw) = get_hardware_random_u64() {
        jitter ^ hw
    } else {
        jitter
    }
}
