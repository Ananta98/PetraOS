// SPDX-License-Identifier: GPL-2.0

//! CRC-32C and CRC-16 checksum primitives for ext4 metadata checksums.
//!
//! Re-exports core checksum primitives relocated to `crate::crypto::hash::crc32c`.

pub use crate::crypto::hash::crc32c::*;
