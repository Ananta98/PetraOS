// SPDX-License-Identifier: GPL-2.0

//! Poly1305 one-time authenticator (RFC 7539 / RFC 8439) in 100% safe Rust.
//!
//! Evaluates the Poly1305 polynomial using 5x 26-bit limbs with 64-bit multi-precision arithmetic.

/// Poly1305 state with five 26-bit limbs for accumulator and clamped key.
#[derive(Clone, Copy, Debug)]
pub struct Poly1305 {
    r: [u32; 5],
    h: [u32; 5],
    pad: [u32; 4],
    leftover: usize,
    buffer: [u8; 16],
}

#[inline(always)]
fn u8to32_le(p: &[u8]) -> u32 {
    u32::from_le_bytes([p[0], p[1], p[2], p[3]])
}

#[inline(always)]
fn u32to8_le(v: u32, out: &mut [u8]) {
    out[0..4].copy_from_slice(&v.to_le_bytes());
}

impl Poly1305 {
    /// Initializes Poly1305 with a 32-byte one-time key.
    pub fn new(key: &[u8; 32]) -> Self {
        // Clamping and loading into 5x 26-bit limbs
        let r0 = u8to32_le(&key[0..4]) & 0x3ffffff;
        let r1 = (u8to32_le(&key[3..7]) >> 2) & 0x3ffff03;
        let r2 = (u8to32_le(&key[6..10]) >> 4) & 0x3ffc0ff;
        let r3 = (u8to32_le(&key[9..13]) >> 6) & 0x3f03fff;
        let r4 = (u8to32_le(&key[12..16]) >> 8) & 0x00fffff;

        let pad0 = u8to32_le(&key[16..20]);
        let pad1 = u8to32_le(&key[20..24]);
        let pad2 = u8to32_le(&key[24..28]);
        let pad3 = u8to32_le(&key[28..32]);

        Self {
            r: [r0, r1, r2, r3, r4],
            h: [0; 5],
            pad: [pad0, pad1, pad2, pad3],
            leftover: 0,
            buffer: [0u8; 16],
        }
    }

    /// Process 16-byte blocks with hibit = (1 << 24).
    fn process_blocks(&mut self, mut m: &[u8], is_final: bool) {
        let hibit = if is_final { 0u32 } else { 1u32 << 24 };

        let r0 = self.r[0] as u64;
        let r1 = self.r[1] as u64;
        let r2 = self.r[2] as u64;
        let r3 = self.r[3] as u64;
        let r4 = self.r[4] as u64;

        let s1 = r1 * 5;
        let s2 = r2 * 5;
        let s3 = r3 * 5;
        let s4 = r4 * 5;

        let mut h0 = self.h[0];
        let mut h1 = self.h[1];
        let mut h2 = self.h[2];
        let mut h3 = self.h[3];
        let mut h4 = self.h[4];

        while m.len() >= 16 {
            // Unpack 16 bytes into 5x 26-bit limbs + hibit
            h0 = h0.wrapping_add(u8to32_le(&m[0..4]) & 0x3ffffff);
            h1 = h1.wrapping_add((u8to32_le(&m[3..7]) >> 2) & 0x3ffffff);
            h2 = h2.wrapping_add((u8to32_le(&m[6..10]) >> 4) & 0x3ffffff);
            h3 = h3.wrapping_add((u8to32_le(&m[9..13]) >> 6) & 0x3ffffff);
            h4 = h4.wrapping_add((u8to32_le(&m[12..16]) >> 8) | hibit);

            // 5x5 limb polynomial multiplication
            let h0_64 = h0 as u64;
            let h1_64 = h1 as u64;
            let h2_64 = h2 as u64;
            let h3_64 = h3 as u64;
            let h4_64 = h4 as u64;

            let d0 = h0_64 * r0 + h1_64 * s4 + h2_64 * s3 + h3_64 * s2 + h4_64 * s1;
            let d1 = h0_64 * r1 + h1_64 * r0 + h2_64 * s4 + h3_64 * s3 + h4_64 * s2;
            let d2 = h0_64 * r2 + h1_64 * r1 + h2_64 * r0 + h3_64 * s4 + h4_64 * s3;
            let d3 = h0_64 * r3 + h1_64 * r2 + h2_64 * r1 + h3_64 * r0 + h4_64 * s4;
            let d4 = h0_64 * r4 + h1_64 * r3 + h2_64 * r2 + h3_64 * r1 + h4_64 * r0;

            // Carry propagation
            let mut c = (d0 >> 26) as u32;
            h0 = (d0 & 0x3ffffff) as u32;

            let d1 = d1 + c as u64;
            c = (d1 >> 26) as u32;
            h1 = (d1 & 0x3ffffff) as u32;

            let d2 = d2 + c as u64;
            c = (d2 >> 26) as u32;
            h2 = (d2 & 0x3ffffff) as u32;

            let d3 = d3 + c as u64;
            c = (d3 >> 26) as u32;
            h3 = (d3 & 0x3ffffff) as u32;

            let d4 = d4 + c as u64;
            c = (d4 >> 26) as u32;
            h4 = (d4 & 0x3ffffff) as u32;

            h0 = h0.wrapping_add(c.wrapping_mul(5));
            c = h0 >> 26;
            h0 &= 0x3ffffff;
            h1 = h1.wrapping_add(c);

            m = &m[16..];
        }

        self.h[0] = h0;
        self.h[1] = h1;
        self.h[2] = h2;
        self.h[3] = h3;
        self.h[4] = h4;
    }

    /// Feeds message bytes into the authenticator.
    pub fn update(&mut self, mut msg: &[u8]) {
        // Handle leftover bytes from previous updates
        if self.leftover > 0 {
            let want = 16 - self.leftover;
            let take = msg.len().min(want);
            self.buffer[self.leftover..self.leftover + take].copy_from_slice(&msg[..take]);
            self.leftover += take;
            msg = &msg[take..];

            if self.leftover < 16 {
                return;
            }

            let block = self.buffer;
            self.process_blocks(&block, false);
            self.leftover = 0;
        }

        // Process full 16-byte blocks directly from input slice
        let full_len = msg.len() & !15;
        if full_len > 0 {
            self.process_blocks(&msg[..full_len], false);
            msg = &msg[full_len..];
        }

        // Store remaining bytes in buffer
        if !msg.is_empty() {
            self.buffer[..msg.len()].copy_from_slice(msg);
            self.leftover = msg.len();
        }
    }

    /// Finalizes the authenticator and returns the 16-byte authentication tag.
    pub fn finalize(mut self) -> [u8; 16] {
        // Process final partial block if present
        if self.leftover > 0 {
            let mut final_block = [0u8; 16];
            final_block[..self.leftover].copy_from_slice(&self.buffer[..self.leftover]);
            final_block[self.leftover] = 1; // Append 0x01 delimiter bit per RFC 7539
            self.process_blocks(&final_block, true);
        }

        // Fully reduce h modulo 2^130 - 5
        let mut h0 = self.h[0];
        let mut h1 = self.h[1];
        let mut h2 = self.h[2];
        let mut h3 = self.h[3];
        let mut h4 = self.h[4];

        let mut c = h1 >> 26;
        h1 &= 0x3ffffff;
        h2 = h2.wrapping_add(c);
        c = h2 >> 26;
        h2 &= 0x3ffffff;
        h3 = h3.wrapping_add(c);
        c = h3 >> 26;
        h3 &= 0x3ffffff;
        h4 = h4.wrapping_add(c);
        c = h4 >> 26;
        h4 &= 0x3ffffff;
        h0 = h0.wrapping_add(c.wrapping_mul(5));
        c = h0 >> 26;
        h0 &= 0x3ffffff;
        h1 = h1.wrapping_add(c);

        // Compute g = h + 5
        let mut g0 = h0.wrapping_add(5);
        c = g0 >> 26;
        g0 &= 0x3ffffff;
        let mut g1 = h1.wrapping_add(c);
        c = g1 >> 26;
        g1 &= 0x3ffffff;
        let mut g2 = h2.wrapping_add(c);
        c = g2 >> 26;
        g2 &= 0x3ffffff;
        let mut g3 = h3.wrapping_add(c);
        c = g3 >> 26;
        g3 &= 0x3ffffff;
        let mut g4 = h4.wrapping_add(c).wrapping_sub(1 << 26);

        // Select h if h < 2^130 - 5, else g
        let mask = (g4 >> 31).wrapping_sub(1);
        g0 &= mask;
        g1 &= mask;
        g2 &= mask;
        g3 &= mask;
        g4 &= mask;
        let not_mask = !mask;
        h0 = (h0 & not_mask) | g0;
        h1 = (h1 & not_mask) | g1;
        h2 = (h2 & not_mask) | g2;
        h3 = (h3 & not_mask) | g3;
        h4 = (h4 & not_mask) | g4;

        // Reconstruct 128-bit integer from 5x 26-bit limbs
        h0 = h0 | (h1 << 26);
        h1 = (h1 >> 6) | (h2 << 20);
        h2 = (h2 >> 12) | (h3 << 14);
        h3 = (h3 >> 18) | (h4 << 8);

        // Add pad modulo 2^128
        let f0 = (h0 as u64) + (self.pad[0] as u64);
        h0 = f0 as u32;
        let f1 = (h1 as u64) + (self.pad[1] as u64) + (f0 >> 32);
        h1 = f1 as u32;
        let f2 = (h2 as u64) + (self.pad[2] as u64) + (f1 >> 32);
        h2 = f2 as u32;
        let f3 = (h3 as u64) + (self.pad[3] as u64) + (f2 >> 32);
        h3 = f3 as u32;

        let mut out = [0u8; 16];
        u32to8_le(h0, &mut out[0..4]);
        u32to8_le(h1, &mut out[4..8]);
        u32to8_le(h2, &mut out[8..12]);
        u32to8_le(h3, &mut out[12..16]);
        out
    }

    /// One-shot Poly1305 MAC generation.
    pub fn mac(key: &[u8; 32], msg: &[u8]) -> [u8; 16] {
        let mut poly = Self::new(key);
        poly.update(msg);
        poly.finalize()
    }
}
