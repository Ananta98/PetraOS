// SPDX-License-Identifier: GPL-2.0

//! vDSO shared data structure (`vvar` page) definition.

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// Clock mode indicating that TSC is unavailable or unstable; fallback to syscall.
pub const VCLOCK_NONE: u32 = 0;
/// Clock mode indicating that high-resolution TSC is active and valid.
pub const VCLOCK_TSC: u32 = 1;

/// Number of base clocks tracked in the vDSO data table.
pub const VDSO_BASES: usize = 4;

/// POSIX timestamp in seconds and nanoseconds.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct VdsoTimestamp {
    pub sec: u64,
    pub nsec: u64,
}

/// Shared timekeeping and clock calibration data page (`vvar`).
///
/// This struct is placed at offset 0 of the read-only `vvar` page, which is mapped
/// directly below the vDSO shared library in every process address space.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct VdsoData {
    /// Seqlock sequence counter. Odd = writer active, Even = consistent data.
    pub seq: u32,
    /// Clock mode (`VCLOCK_NONE` or `VCLOCK_TSC`).
    pub clock_mode: u32,
    /// Hardware cycle counter reading at the time base timestamps were sampled.
    pub cycle_last: u64,
    /// Bitmask for cycle counter rollover (typically `!0u64`).
    pub mask: u64,
    /// Multiplier for fixed-point nanosecond scaling: `(delta * mult) >> shift`.
    pub mult: u32,
    /// Shift factor for fixed-point nanosecond scaling.
    pub shift: u32,
    /// Base timestamps for supported clock IDs:
    /// - `[0]`: `CLOCK_REALTIME` / `CLOCK_REALTIME_ALARM`
    /// - `[1]`: `CLOCK_MONOTONIC` / `CLOCK_MONOTONIC_RAW`
    /// - `[2]`: `CLOCK_BOOTTIME` / `CLOCK_BOOTTIME_ALARM`
    /// - `[3]`: `CLOCK_TAI`
    pub basetime: [VdsoTimestamp; VDSO_BASES],
    /// Coarse wall-clock timestamp for `CLOCK_REALTIME_COARSE`.
    pub coarse_realtime: VdsoTimestamp,
    /// Coarse monotonic timestamp for `CLOCK_MONOTONIC_COARSE`.
    pub coarse_monotonic: VdsoTimestamp,
}
