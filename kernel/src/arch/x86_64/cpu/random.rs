use core::sync::atomic::{AtomicU64, Ordering};

/// Read the current CPU Time-Stamp Counter (TSC).
#[inline(always)]
pub fn rdtsc() -> u64 {
    // SAFETY: Reading the CPU time-stamp counter on x86_64 is always safe.
    unsafe { core::arch::x86_64::_rdtsc() }
}

static RANDOM_STATE: AtomicU64 = AtomicU64::new(0x853c_49e6_748f_ea9b);

/// Generates a pseudo-random 64-bit unsigned integer with TSC entropy mixing.
pub fn next_random_u64() -> u64 {
    let tsc = rdtsc();
    let mut state = RANDOM_STATE.load(Ordering::Relaxed);
    if state == 0 {
        state = tsc | 1;
    }
    state ^= state << 13;
    state ^= state >> 7;
    state ^= state << 17;
    state = state.wrapping_add(tsc);
    RANDOM_STATE.store(state, Ordering::Relaxed);
    state
}

/// Fills the given buffer with random bytes generated from TSC-seeded PRNG.
pub fn fill_random_bytes(buf: &mut [u8]) {
    let mut chunks = buf.chunks_exact_mut(8);
    for chunk in chunks.by_ref() {
        let rand_val = next_random_u64();
        chunk.copy_from_slice(&rand_val.to_ne_bytes());
    }
    let remainder = chunks.into_remainder();
    if !remainder.is_empty() {
        let rand_val = next_random_u64();
        let bytes = rand_val.to_ne_bytes();
        remainder.copy_from_slice(&bytes[..remainder.len()]);
    }
}
