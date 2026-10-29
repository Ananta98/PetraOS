// SPDX-License-Identifier: GPL-2.0

//! Multi-source hardware & kernel entropy harvester.

pub mod jitter;
pub mod rdrand;

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

pub use jitter::get_jitter_entropy_u64;
pub use rdrand::get_hardware_random_u64;

static INITIALIZED: AtomicBool = AtomicBool::new(false);
static ENTROPY_BITS: AtomicU32 = AtomicU32::new(0);

/// Minimum entropy bits required to consider the primary entropy pool initialized.
pub const MIN_INITIAL_ENTROPY_BITS: u32 = 256;

/// Returns `true` if the entropy pool has accumulated sufficient entropy.
pub fn is_initialized() -> bool {
    INITIALIZED.load(Ordering::Acquire)
}

/// Marks the entropy pool as initialized.
pub fn mark_initialized() {
    INITIALIZED.store(true, Ordering::Release);
}

/// Harvests combined entropy from hardware RDRAND (if available) and CPU/scheduler jitter.
pub fn harvest_entropy_u64() -> u64 {
    let jitter = get_jitter_entropy_u64();
    if let Some(hw) = get_hardware_random_u64() {
        INITIALIZED.store(true, Ordering::Release);
        jitter ^ hw
    } else {
        let bits = ENTROPY_BITS.fetch_add(8, Ordering::Relaxed);
        if bits + 8 >= MIN_INITIAL_ENTROPY_BITS {
            INITIALIZED.store(true, Ordering::Release);
        }
        jitter
    }
}
