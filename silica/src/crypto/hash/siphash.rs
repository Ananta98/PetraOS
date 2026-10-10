// SPDX-License-Identifier: GPL-2.0

//! SipHash-2-4 implementation in 100% safe Rust.
//!
//! SipHash is a cryptographically secure, keyed PRF optimized for short inputs.
//! It is used for hash tables to prevent HashDoS algorithmic complexity attacks
//! and for generating unpredictable internal identifiers.

/// SipHash-2-4 state with four 64-bit words.
#[derive(Clone, Copy, Debug)]
pub struct SipHash24 {
    v0: u64,
    v1: u64,
    v2: u64,
    v3: u64,
}

#[inline(always)]
fn sipround(v0: &mut u64, v1: &mut u64, v2: &mut u64, v3: &mut u64) {
    *v0 = v0.wrapping_add(*v1);
    *v1 = v1.rotate_left(13) ^ *v0;
    *v0 = v0.rotate_left(32);

    *v2 = v2.wrapping_add(*v3);
    *v3 = v3.rotate_left(16) ^ *v2;

    *v0 = v0.wrapping_add(*v3);
    *v3 = v3.rotate_left(21) ^ *v0;

    *v2 = v2.wrapping_add(*v1);
    *v1 = v1.rotate_left(17) ^ *v2;
    *v2 = v2.rotate_left(32);
}

impl SipHash24 {
    /// Creates a new SipHash-2-4 hasher initialized with a 128-bit key (`k0`, `k1`).
    pub const fn new(k0: u64, k1: u64) -> Self {
        Self {
            v0: k0 ^ 0x736f6d6570736575,
            v1: k1 ^ 0x646f72616e646f6d,
            v2: k0 ^ 0x6c7967656e657261,
            v3: k1 ^ 0x7465646279746573,
        }
    }

    /// Computes the 64-bit SipHash-2-4 tag of `data` using key (`k0`, `k1`).
    pub fn hash(k0: u64, k1: u64, data: &[u8]) -> u64 {
        let mut hasher = Self::new(k0, k1);
        let len = data.len();
        let chunks = len / 8;

        for i in 0..chunks {
            let offset = i * 8;
            let m = u64::from_le_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
                data[offset + 4],
                data[offset + 5],
                data[offset + 6],
                data[offset + 7],
            ]);
            hasher.v3 ^= m;
            sipround(
                &mut hasher.v0,
                &mut hasher.v1,
                &mut hasher.v2,
                &mut hasher.v3,
            );
            sipround(
                &mut hasher.v0,
                &mut hasher.v1,
                &mut hasher.v2,
                &mut hasher.v3,
            );
            hasher.v0 ^= m;
        }

        // Process remaining tail bytes (0..7 bytes) + byte length in the highest byte
        let rem = len % 8;
        let mut last = ((len as u64) & 0xff) << 56;
        let offset = chunks * 8;
        for i in 0..rem {
            last |= (data[offset + i] as u64) << (i * 8);
        }

        hasher.v3 ^= last;
        sipround(
            &mut hasher.v0,
            &mut hasher.v1,
            &mut hasher.v2,
            &mut hasher.v3,
        );
        sipround(
            &mut hasher.v0,
            &mut hasher.v1,
            &mut hasher.v2,
            &mut hasher.v3,
        );
        hasher.v0 ^= last;

        // Finalization: 4 rounds
        hasher.v2 ^= 0xff;
        sipround(
            &mut hasher.v0,
            &mut hasher.v1,
            &mut hasher.v2,
            &mut hasher.v3,
        );
        sipround(
            &mut hasher.v0,
            &mut hasher.v1,
            &mut hasher.v2,
            &mut hasher.v3,
        );
        sipround(
            &mut hasher.v0,
            &mut hasher.v1,
            &mut hasher.v2,
            &mut hasher.v3,
        );
        sipround(
            &mut hasher.v0,
            &mut hasher.v1,
            &mut hasher.v2,
            &mut hasher.v3,
        );

        hasher.v0 ^ hasher.v1 ^ hasher.v2 ^ hasher.v3
    }
}
