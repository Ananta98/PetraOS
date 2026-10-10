// SPDX-License-Identifier: GPL-2.0

//! Standalone ChaCha20 cipher block and stream generator (RFC 7539 / RFC 8439).

/// ChaCha20 constant "expand 32-byte k" in little-endian 32-bit words.
pub const CHACHA_CONSTANTS: [u32; 4] = [0x61707865, 0x3320646e, 0x79622d32, 0x6b206574];

/// ChaCha20 quarter round operation (RFC 7539 Section 2.1).
#[inline(always)]
pub fn quarter_round(state: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    state[a] = state[a].wrapping_add(state[b]);
    state[d] ^= state[a];
    state[d] = state[d].rotate_left(16);

    state[c] = state[c].wrapping_add(state[d]);
    state[b] ^= state[c];
    state[b] = state[b].rotate_left(12);

    state[a] = state[a].wrapping_add(state[b]);
    state[d] ^= state[a];
    state[d] = state[d].rotate_left(8);

    state[c] = state[c].wrapping_add(state[d]);
    state[b] ^= state[c];
    state[b] = state[b].rotate_left(7);
}

/// ChaCha20 core block function producing 64 bytes from a 16-word state matrix (RFC 7539 Section 2.3).
pub fn chacha20_block(state: &[u32; 16]) -> [u8; 64] {
    let mut working = *state;

    // 10 double rounds (20 rounds total)
    for _ in 0..10 {
        // Column rounds
        quarter_round(&mut working, 0, 4, 8, 12);
        quarter_round(&mut working, 1, 5, 9, 13);
        quarter_round(&mut working, 2, 6, 10, 14);
        quarter_round(&mut working, 3, 7, 11, 15);

        // Diagonal rounds
        quarter_round(&mut working, 0, 5, 10, 15);
        quarter_round(&mut working, 1, 6, 11, 12);
        quarter_round(&mut working, 2, 7, 8, 13);
        quarter_round(&mut working, 3, 4, 9, 14);
    }

    let mut out = [0u8; 64];
    for i in 0..16 {
        let val = working[i].wrapping_add(state[i]);
        out[i * 4..i * 4 + 4].copy_from_slice(&val.to_le_bytes());
    }
    out
}
