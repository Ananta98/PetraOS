// SPDX-License-Identifier: GPL-2.0

//! Device number encoding macros (`makedev`, `major`, `minor`).

/// Forms a 64-bit device number from major and minor parts (Linux ABI).
#[inline]
pub const fn makedev(maj: u32, min: u32) -> u64 {
    ((maj as u64 & 0xfff) << 8)
        | ((min as u64 & 0xff) << 0)
        | ((maj as u64 & !0xfff) << 32)
        | ((min as u64 & !0xff) << 12)
}

/// Extracts the major device number from a 64-bit device ID.
#[inline]
pub const fn major(dev: u64) -> u32 {
    (((dev >> 8) & 0xfff) | ((dev >> 32) & !0xfff)) as u32
}

/// Extracts the minor device number from a 64-bit device ID.
#[inline]
pub const fn minor(dev: u64) -> u32 {
    ((dev & 0xff) | ((dev >> 12) & !0xff)) as u32
}
