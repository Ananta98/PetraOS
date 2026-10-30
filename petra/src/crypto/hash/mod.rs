// SPDX-License-Identifier: GPL-2.0

//! Digest and checksum algorithms (zero-allocation / no_std).

pub mod crc32c;
pub mod sha256;
pub mod siphash;

pub use crc32c::{crc16, crc32c, crc32c_finalize, crc32c_one};
pub use sha256::Sha256;
pub use siphash::SipHash24;
