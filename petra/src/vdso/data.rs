// SPDX-License-Identifier: GPL-2.0

//! vDSO shared data structure (`vvar` page) definition.
//!
//! Layout matches Linux kernel v6.2.10 datapage layout for x86_64:
//! <https://elixir.bootlin.com/linux/v6.2.10/source/include/vdso/datapage.h#L90>

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// Clock mode indicating that TSC is unavailable or unstable; fallback to syscall.
pub const VCLOCK_NONE: i32 = 0;
/// Clock mode indicating that high-resolution TSC is active and valid.
pub const VCLOCK_TSC: i32 = 1;

/// POSIX Clock IDs supported in the Linux vDSO.
pub const CLOCK_REALTIME: usize = 0;
pub const CLOCK_MONOTONIC: usize = 1;
pub const CLOCK_MONOTONIC_RAW: usize = 4;
pub const CLOCK_REALTIME_COARSE: usize = 5;
pub const CLOCK_MONOTONIC_COARSE: usize = 6;
pub const CLOCK_BOOTTIME: usize = 7;
pub const CLOCK_TAI: usize = 11;

/// Number of base clocks tracked in the vDSO data table (up to `CLOCK_TAI + 1`).
pub const VDSO_BASES: usize = 12;

/// POSIX timestamp in seconds and nanoseconds (or shifted nanoseconds for high-res clocks).
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct VdsoInstant {
    pub sec: u64,
    pub nsec_info: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct ArchVdsoData {}

/// Plain-old-data vDSO data page structure.
///
/// Layout matches Linux v6.2.10 `struct vdso_data`:
/// - `seq`: sequence counter for lockless reading
/// - `clock_mode`: clocksource identifier (`VCLOCK_NONE`, `VCLOCK_TSC`)
/// - `cycle_last`: TSC value at last calibration
/// - `mask`: rollover bitmask
/// - `mult`: fixed-point nanosecond multiplier
/// - `shift`: fixed-point nanosecond shift
/// - `basetime`: base timestamps for clock IDs
/// - `tz_minuteswest` / `tz_dsttime`: timezone offsets
/// - `hrtimer_res`: high-resolution timer resolution in nanoseconds
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct VdsoData {
    pub seq: u32,
    pub clock_mode: i32,
    pub cycle_last: u64,
    pub mask: u64,
    pub mult: u32,
    pub shift: u32,
    pub basetime: [VdsoInstant; VDSO_BASES],
    pub tz_minuteswest: i32,
    pub tz_dsttime: i32,
    pub hrtimer_res: u32,
    pub __unused: u32,
    pub arch_data: ArchVdsoData,
}

const _: () = {
    assert!(core::mem::size_of::<VdsoData>() == 240);
    assert!(core::mem::offset_of!(VdsoData, hrtimer_res) == 232);
    assert!(core::mem::offset_of!(VdsoData, basetime) == 32);
};
