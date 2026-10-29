// SPDX-License-Identifier: GPL-2.0

//! Cryptographic services, stream ciphers, hashing, and random number generation.

pub mod cipher;
pub mod entropy;
pub mod hash;
pub mod random;

pub use cipher::{CHACHA_CONSTANTS, Poly1305, chacha20_block, quarter_round};
pub use entropy::{get_hardware_random_u64, get_jitter_entropy_u64, harvest_entropy_u64};
pub use hash::{Sha256, SipHash24};
pub use random::*;
