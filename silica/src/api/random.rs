// SPDX-License-Identifier: GPL-2.0

//! Random generator ABI definitions.
//!
//! Defines flags for `getrandom(2)`.

/// Return immediately with `EAGAIN` if the requested bytes cannot be served
/// without blocking.
pub const GRND_NONBLOCK: u32 = 0x0001;

/// Use the `/dev/random` true-random source instead of `/dev/urandom`.
pub const GRND_RANDOM: u32 = 0x0002;

/// Return random bytes even if the entropy pool has not yet initialized.
pub const GRND_INSECURE: u32 = 0x0004;

/// Bitmask of all supported flags for `getrandom(2)`.
pub const GRND_VALID_FLAGS_MASK: u32 = GRND_NONBLOCK | GRND_RANDOM | GRND_INSECURE;
