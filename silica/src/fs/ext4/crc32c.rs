// SPDX-License-Identifier: GPL-2.0

//! CRC-32C and CRC-16 checksum primitives for ext4 metadata checksums.
//!
//! Ext4 (`metadata_csum`) uses CRC-32C as a *chaining* function: the caller
//! feeds pieces of the structure one after another to the same running state
//! (`0xFFFFFFFF` seeded), and the final result is used as-is (32-bit) or
//! truncated to its lower 16 bits (legacy 16-bit fields). This mirrors the
//! kernel's `ext4_chksum`/`jbd2_chksum` API, which is why the post-inversion
//! XOR of `0xFFFFFFFF` is applied only in a "finalize" convenience.

/// CRC-32C (Castagnoli) reflected polynomial.
const CRC32C_POLY: u32 = 0x82F63B78;

/// Standard CRC-16/CCITT (used by the legacy `gdt_csum` feature).
const CRC16_POLY: u16 = 0x1021;

/// Builds the byte-wise CRC-32C lookup table at compile time.
const fn crc32c_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut c = i as u32;
        let mut j = 0;
        while j < 8 {
            c = if c & 1 != 0 {
                CRC32C_POLY ^ (c >> 1)
            } else {
                c >> 1
            };
            j += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}

/// CRC-32C byte-wise lookup table.
static CRC32C_TABLE: [u32; 256] = crc32c_table();

/// Feeds `data` into a running CRC-32C state and returns the new state.
///
/// Start a checksum with `0xFFFFFFFF`; the state passed back and forth is the
/// raw register value (no inversion), matching `ext4_chksum` semantics.
#[must_use]
#[expect(clippy::index_refutable_slice)]
pub fn crc32c(mut state: u32, data: &[u8]) -> u32 {
    let table = &CRC32C_TABLE;
    for &b in data {
        state = (state >> 8) ^ table[((state ^ b as u32) & 0xFF) as usize];
    }
    state
}

/// Feeds `data` into a CRC-16/CCITT running state (`0xFFFF` seed, MSB-first).
#[must_use]
pub fn crc16(mut state: u16, data: &[u8]) -> u16 {
    for &b in data {
        state ^= (b as u16) << 8;
        for _ in 0..8 {
            state = if state & 0x8000 != 0 {
                (state << 1) ^ CRC16_POLY
            } else {
                state << 1
            };
        }
    }
    state
}

/// Completes a CRC-32C value (applies the final `0xFFFFFFFF` inversion).
#[must_use]
pub const fn crc32c_finalize(state: u32) -> u32 {
    state ^ 0xFFFF_FFFF
}

/// Full one-shot CRC-32C (equivalent to `crc32c_finalize(crc32c(0xFFFFFFFF, data))`).
#[must_use]
pub fn crc32c_one(data: &[u8]) -> u32 {
    crc32c_finalize(crc32c(0xFFFF_FFFF, data))
}
