// SPDX-License-Identifier: GPL-2.0

//! CRC-32C (Castagnoli) and CRC-16/CCITT checksum primitives.
//!
//! Provides zero-allocation, compile-time lookup table backed CRC calculations
//! used across filesystems (ext4, btrfs), network protocols (SCTP, iSCSI), and storage integrity.

/// CRC-32C (Castagnoli) reflected polynomial.
const CRC32C_POLY: u32 = 0x82F63B78;

/// Standard CRC-16/CCITT polynomial.
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
/// raw register value (no inversion), matching chaining semantics.
#[must_use]
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
