// SPDX-License-Identifier: GPL-2.0

//! Time and clock related system calls.

pub mod clock;
pub mod gettimeofday;
pub mod nanosleep;
pub mod times;

pub use clock::*;
pub use gettimeofday::*;
pub use nanosleep::*;
pub use times::*;

/// Linux x86_64 compatible `struct timespec` (16 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Timespec {
    pub tv_sec: i64,
    pub tv_nsec: i64,
}

impl Timespec {
    pub fn to_bytes(&self) -> [u8; 16] {
        let mut buf = [0u8; 16];
        buf[0..8].copy_from_slice(&self.tv_sec.to_ne_bytes());
        buf[8..16].copy_from_slice(&self.tv_nsec.to_ne_bytes());
        buf
    }

    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        if buf.len() < 16 {
            return None;
        }
        let tv_sec = i64::from_ne_bytes(buf[0..8].try_into().ok()?);
        let tv_nsec = i64::from_ne_bytes(buf[8..16].try_into().ok()?);
        Some(Self { tv_sec, tv_nsec })
    }
}

/// Linux x86_64 compatible `struct timeval` (16 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Timeval {
    pub tv_sec: i64,
    pub tv_usec: i64,
}

impl Timeval {
    pub fn to_bytes(&self) -> [u8; 16] {
        let mut buf = [0u8; 16];
        buf[0..8].copy_from_slice(&self.tv_sec.to_ne_bytes());
        buf[8..16].copy_from_slice(&self.tv_usec.to_ne_bytes());
        buf
    }

    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        if buf.len() < 16 {
            return None;
        }
        let tv_sec = i64::from_ne_bytes(buf[0..8].try_into().ok()?);
        let tv_usec = i64::from_ne_bytes(buf[8..16].try_into().ok()?);
        Some(Self { tv_sec, tv_usec })
    }
}

/// Returns the wall-clock epoch timestamp at boot in seconds since UNIX epoch.
pub fn get_boot_epoch_secs() -> u64 {
    // Standard baseline epoch timestamp (e.g., 2026-04-07 00:00:00 UTC)
    1775563440
}
