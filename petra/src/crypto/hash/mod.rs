// SPDX-License-Identifier: GPL-2.0

//! Digest algorithms (zero-allocation / no_std).

pub mod sha256;
pub mod siphash;

pub use sha256::Sha256;
pub use siphash::SipHash24;
