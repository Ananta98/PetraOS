// SPDX-License-Identifier: GPL-2.0

//! Symmetric encryption & message authentication primitives.

pub mod aead;
pub mod chacha20;
pub mod poly1305;

pub use aead::ChaCha20Poly1305;
pub use chacha20::{CHACHA_CONSTANTS, chacha20_block, chacha20_crypt, quarter_round};
pub use poly1305::Poly1305;
