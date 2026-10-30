// SPDX-License-Identifier: GPL-2.0

//! ChaCha20-Poly1305 Authenticated Encryption with Associated Data (AEAD - RFC 8439).
//!
//! Standard AEAD construction used by WireGuard, TLS 1.3, and SSH.

use crate::crypto::cipher::{
    chacha20::{CHACHA_CONSTANTS, chacha20_block, chacha20_crypt},
    poly1305::Poly1305,
};
use crate::uapi::errno::{Errno, Result};

pub struct ChaCha20Poly1305;

impl ChaCha20Poly1305 {
    /// Generates the 32-byte Poly1305 one-time key for a given key and nonce.
    fn poly1305_key(key: &[u8; 32], nonce: &[u8; 12]) -> [u8; 32] {
        let mut state = [0u32; 16];
        state[0..4].copy_from_slice(&CHACHA_CONSTANTS);
        for i in 0..8 {
            state[4 + i] = u32::from_le_bytes([
                key[i * 4],
                key[i * 4 + 1],
                key[i * 4 + 2],
                key[i * 4 + 3],
            ]);
        }
        state[12] = 0; // counter = 0
        for i in 0..3 {
            state[13 + i] = u32::from_le_bytes([
                nonce[i * 4],
                nonce[i * 4 + 1],
                nonce[i * 4 + 2],
                nonce[i * 4 + 3],
            ]);
        }
        let block = chacha20_block(&state);
        let mut otk = [0u8; 32];
        otk.copy_from_slice(&block[0..32]);
        otk
    }

    /// Computes the Poly1305 MAC over (AAD || padding || ciphertext || padding || len(AAD) || len(ciphertext)).
    fn compute_mac(otk: &[u8; 32], aad: &[u8], ciphertext: &[u8]) -> [u8; 16] {
        let mut poly = Poly1305::new(otk);
        poly.update(aad);
        if aad.len() % 16 != 0 {
            let pad = [0u8; 16];
            poly.update(&pad[..16 - (aad.len() % 16)]);
        }
        poly.update(ciphertext);
        if ciphertext.len() % 16 != 0 {
            let pad = [0u8; 16];
            poly.update(&pad[..16 - (ciphertext.len() % 16)]);
        }
        let aad_len = (aad.len() as u64).to_le_bytes();
        let ct_len = (ciphertext.len() as u64).to_le_bytes();
        poly.update(&aad_len);
        poly.update(&ct_len);
        poly.finalize()
    }

    /// Encrypts `data` in-place and returns the 16-byte Poly1305 authentication tag.
    pub fn encrypt_in_place(
        key: &[u8; 32],
        nonce: &[u8; 12],
        aad: &[u8],
        data: &mut [u8],
    ) -> [u8; 16] {
        let otk = Self::poly1305_key(key, nonce);
        // Encrypt with counter = 1
        chacha20_crypt(key, nonce, 1, data);
        Self::compute_mac(&otk, aad, data)
    }

    /// Authenticates `data` (ciphertext) with `expected_tag`, and if valid, decrypts in-place.
    pub fn decrypt_in_place(
        key: &[u8; 32],
        nonce: &[u8; 12],
        aad: &[u8],
        data: &mut [u8],
        expected_tag: &[u8; 16],
    ) -> Result<()> {
        let otk = Self::poly1305_key(key, nonce);
        let computed_tag = Self::compute_mac(&otk, aad, data);

        // Constant-time comparison
        let mut diff = 0u8;
        for i in 0..16 {
            diff |= computed_tag[i] ^ expected_tag[i];
        }
        if diff != 0 {
            return Err(Errno::EACCES);
        }

        // Decrypt with counter = 1
        chacha20_crypt(key, nonce, 1, data);
        Ok(())
    }
}
