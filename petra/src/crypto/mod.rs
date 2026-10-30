// SPDX-License-Identifier: GPL-2.0

//! Cryptographic services, stream ciphers, hashing, and random number generation.

pub mod cipher;
pub mod entropy;
pub mod hash;
pub mod random;

pub use cipher::{
    CHACHA_CONSTANTS, ChaCha20Poly1305, Poly1305, chacha20_block, chacha20_crypt, quarter_round,
};
pub use entropy::{get_hardware_random_u64, get_jitter_entropy_u64, harvest_entropy_u64};
pub use hash::{Sha256, SipHash24, crc16, crc32c, crc32c_finalize, crc32c_one};
pub use random::*;
