// SPDX-License-Identifier: GPL-2.0

//! Cryptographically Secure Pseudo-Random Number Generator (CSPRNG).
//!
//! Implements a ChaCha20-based stream generator in 100% safe Rust, continuously seeded
//! with high-resolution system timers, CPU jitter, and scheduler states.

use crate::crypto::cipher::chacha20::{CHACHA_CONSTANTS, chacha20_block};
use crate::crypto::entropy::harvest_entropy_u64;
use ostd::sync::SpinLock;

/// ChaCha20 CSPRNG state.
pub struct ChaChaRng {
    state: [u32; 16],
    buffer: [u8; 64],
    buf_idx: usize,
}

impl ChaChaRng {
    /// Initializes the generator with initial entropy.
    pub fn new() -> Self {
        let mut rng = Self {
            state: [0; 16],
            buffer: [0; 64],
            buf_idx: 64,
        };

        rng.state[0..4].copy_from_slice(&CHACHA_CONSTANTS);
        rng.reseed(0xdeadbeef_cafebabe);
        rng
    }

    /// Injects fresh entropy into key and nonce words.
    pub fn reseed(&mut self, extra_entropy: u64) {
        let harvested = harvest_entropy_u64() ^ extra_entropy;
        let elapsed = ostd::timer::Jiffies::elapsed().as_duration();
        let nanos = elapsed.as_nanos() as u64;
        let sched_ticks = crate::sched::now();

        self.state[4] ^= nanos as u32;
        self.state[5] ^= (nanos >> 32) as u32;
        self.state[6] ^= sched_ticks as u32;
        self.state[7] ^= (sched_ticks >> 32) as u32;
        self.state[8] ^= harvested as u32;
        self.state[9] ^= (harvested >> 32) as u32;
        self.state[10] ^= 0x5a5a5a5a;
        self.state[11] ^= 0xa5a5a5a5;
        self.state[12] = 0; // counter low
        self.state[13] = 0; // counter high
        self.state[14] ^= 0x12345678;
        self.state[15] ^= 0x9abcdef0;

        self.buf_idx = 64;
    }

    /// Refills the internal 64-byte random buffer.
    fn refill(&mut self) {
        self.buffer = chacha20_block(&self.state);
        self.buf_idx = 0;

        // Increment 64-bit block counter
        self.state[12] = self.state[12].wrapping_add(1);
        if self.state[12] == 0 {
            self.state[13] = self.state[13].wrapping_add(1);
        }
    }

    /// Fills `dest` with random bytes.
    pub fn fill_bytes(&mut self, mut dest: &mut [u8]) {
        while !dest.is_empty() {
            if self.buf_idx >= 64 {
                self.refill();
            }

            let avail = 64 - self.buf_idx;
            let take = dest.len().min(avail);
            dest[..take].copy_from_slice(&self.buffer[self.buf_idx..self.buf_idx + take]);
            self.buf_idx += take;
            dest = &mut dest[take..];
        }
    }
}

/// Global kernel CSPRNG instance.
static CSPRNG: SpinLock<ChaChaRng> = SpinLock::new(ChaChaRng {
    state: [
        0x61707865, 0x3320646e, 0x79622d32, 0x6b206574, 0x12345678, 0x9abcdef0, 0x0fedcba9,
        0x87654321, 0xdeadbeef, 0xcafebabe, 0x5a5a5a5a, 0xa5a5a5a5, 0, 0, 0x33333333, 0x77777777,
    ],
    buffer: [0; 64],
    buf_idx: 64,
});

/// Fills `buf` with cryptographically secure random bytes.
pub fn get_random_bytes(buf: &mut [u8]) {
    let mut rng = CSPRNG.lock();
    // Mix in fresh timer jitter on each request
    let jitter = crate::sched::now();
    rng.reseed(jitter);
    rng.fill_bytes(buf);
}

/// Returns whether the CSPRNG entropy pool is initialized.
pub fn is_initialized() -> bool {
    crate::crypto::entropy::is_initialized()
}

/// Ensures that the entropy pool is initialized, actively harvesting if needed.
pub fn ensure_initialized() {
    if !is_initialized() {
        let mut rng = CSPRNG.lock();
        for _ in 0..32 {
            let entropy = harvest_entropy_u64();
            rng.reseed(entropy);
            if crate::crypto::entropy::is_initialized() {
                break;
            }
        }
        crate::crypto::entropy::mark_initialized();
    }
}

/// Mixes external entropy into the CSPRNG state and updates initialization status.
pub fn add_entropy(data: &[u8]) {
    if data.is_empty() {
        return;
    }
    let digest = crate::crypto::hash::Sha256::digest(data);
    let mut rng = CSPRNG.lock();
    for i in 0..4 {
        let word = u32::from_le_bytes([
            digest[i * 4],
            digest[i * 4 + 1],
            digest[i * 4 + 2],
            digest[i * 4 + 3],
        ]);
        rng.state[8 + i] ^= word;
    }
    rng.buf_idx = 64;
    crate::crypto::entropy::mark_initialized();
}

/// Initializes the global CSPRNG with initial hardware and jitter entropy.
pub fn init() {
    let mut rng = CSPRNG.lock();
    let initial_entropy = harvest_entropy_u64();
    rng.reseed(initial_entropy);
    ensure_initialized();
}
