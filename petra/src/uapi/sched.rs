// SPDX-License-Identifier: GPL-2.0

//! Scheduler ABI definitions.
//!
//! Defines scheduling policies (`SCHED_OTHER`, `SCHED_FIFO`, `SCHED_RR`, etc.),
//! `struct sched_param`, `struct sched_attr`, and CPU affinity bitmask definitions.

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// Default time-sharing policy (EEVDF in PetraOS / Linux).
pub const SCHED_OTHER: i32 = 0;
/// Alias for `SCHED_OTHER`.
pub const SCHED_NORMAL: i32 = 0;
/// Real-time First-In, First-Out policy.
pub const SCHED_FIFO: i32 = 1;
/// Real-time Round-Robin policy.
pub const SCHED_RR: i32 = 2;
/// Batch style execution policy.
pub const SCHED_BATCH: i32 = 3;
/// Reserved policy.
pub const SCHED_ISO: i32 = 4;
/// Very low priority background policy.
pub const SCHED_IDLE: i32 = 5;
/// Deadline scheduling policy.
pub const SCHED_DEADLINE: i32 = 6;

/// Policy modifier flag: child processes do not inherit privileged scheduling policies.
pub const SCHED_RESET_ON_FORK: i32 = 0x40000000;

/// `sched_attr` flag: reset on fork.
pub const SCHED_FLAG_RESET_ON_FORK: u64 = 0x01;
/// `sched_attr` flag: reclaim capacity.
pub const SCHED_FLAG_RECLAIM: u64 = 0x02;
/// `sched_attr` flag: deadline overrun signal.
pub const SCHED_FLAG_DL_OVERRUN: u64 = 0x04;
/// `sched_attr` flag: keep existing policy.
pub const SCHED_FLAG_KEEP_POLICY: u64 = 0x08;
/// `sched_attr` flag: keep existing parameters.
pub const SCHED_FLAG_KEEP_PARAMS: u64 = 0x10;
/// `sched_attr` flag: utilize estimation.
pub const SCHED_FLAG_UTIL_EST: u64 = 0x20;

/// Minimum static priority for real-time policies (`SCHED_FIFO`, `SCHED_RR`).
pub const RT_PRIO_MIN: i32 = 1;
/// Maximum static priority for real-time policies (`SCHED_FIFO`, `SCHED_RR`).
pub const RT_PRIO_MAX: i32 = 99;

/// Number of CPUs represented in a standard glibc `cpu_set_t`.
pub const CPU_SETSIZE: usize = 1024;
/// Number of bytes in a standard `cpu_set_t`.
pub const CPU_BYTES: usize = CPU_SETSIZE / 8; // 128 bytes

/// Linux compatible `struct sched_param` (4 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct SchedParam {
    /// Scheduling priority (0 for normal policies, 1..99 for real-time).
    pub sched_priority: i32,
}

impl SchedParam {
    pub const fn new(sched_priority: i32) -> Self {
        Self { sched_priority }
    }

    /// Serializes `SchedParam` to a 4-byte array without unsafe code.
    pub fn to_bytes(&self) -> [u8; 4] {
        zerocopy::transmute!(*self)
    }

    /// Deserializes `SchedParam` from a byte slice safely.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        Self::read_from_bytes(bytes).ok()
    }
}

/// Linux compatible `struct sched_attr` (56 bytes minimum).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct SchedAttr {
    /// Size of this structure in bytes (for versioning).
    pub size: u32,
    /// Policy (`SCHED_*`).
    pub sched_policy: u32,
    /// Flags (`SCHED_FLAG_*`).
    pub sched_flags: u64,
    /// Nice value (`-20..=19` for `SCHED_OTHER` / `SCHED_BATCH`).
    pub sched_nice: i32,
    /// Static priority (`0` or `1..99`).
    pub sched_priority: u32,
    /// Runtime in nanoseconds for deadline scheduling.
    pub sched_runtime: u64,
    /// Deadline in nanoseconds for deadline scheduling.
    pub sched_deadline: u64,
    /// Period in nanoseconds for deadline scheduling.
    pub sched_period: u64,
    /// Minimum utilization clamp (SCHED_FLAG_UTIL_CLAMP).
    pub sched_util_min: u32,
    /// Maximum utilization clamp (SCHED_FLAG_UTIL_CLAMP).
    pub sched_util_max: u32,
}

impl SchedAttr {
    pub const MIN_SIZE: usize = 48;
    pub const FULL_SIZE: usize = 56;

    /// Serializes `SchedAttr` to a 56-byte array without unsafe code.
    pub fn to_bytes(&self) -> [u8; 56] {
        zerocopy::transmute!(*self)
    }

    /// Deserializes `SchedAttr` from a byte slice safely.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < Self::MIN_SIZE {
            return None;
        }
        let mut buf = [0u8; Self::FULL_SIZE];
        let copy_len = bytes.len().min(Self::FULL_SIZE);
        buf[..copy_len].copy_from_slice(&bytes[..copy_len]);
        Self::read_from_bytes(&buf).ok()
    }
}

/// A 1024-bit CPU affinity bitmask matching glibc `cpu_set_t` (128 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct CpuMask {
    pub bits: [u64; 16],
}

impl Default for CpuMask {
    fn default() -> Self {
        Self { bits: [0u64; 16] }
    }
}

impl CpuMask {
    /// Creates an empty CPU mask.
    pub const fn new() -> Self {
        Self { bits: [0u64; 16] }
    }

    /// Creates a CPU mask with all CPUs enabled.
    pub const fn all() -> Self {
        Self {
            bits: [u64::MAX; 16],
        }
    }

    /// Tests if a CPU bit is set.
    pub fn is_set(&self, cpu: usize) -> bool {
        if cpu >= CPU_SETSIZE {
            return false;
        }
        let word = cpu / 64;
        let bit = cpu % 64;
        (self.bits[word] & (1 << bit)) != 0
    }

    /// Sets a CPU bit.
    pub fn set(&mut self, cpu: usize) {
        if cpu < CPU_SETSIZE {
            let word = cpu / 64;
            let bit = cpu % 64;
            self.bits[word] |= 1 << bit;
        }
    }

    /// Clears a CPU bit.
    pub fn clear(&mut self, cpu: usize) {
        if cpu < CPU_SETSIZE {
            let word = cpu / 64;
            let bit = cpu % 64;
            self.bits[word] &= !(1 << bit);
        }
    }

    /// Returns `true` if no CPUs are set in the mask.
    pub fn is_empty(&self) -> bool {
        self.bits.iter().all(|&w| w == 0)
    }

    /// Serializes the CPU mask to a 128-byte array.
    pub fn to_bytes(&self) -> [u8; 128] {
        zerocopy::transmute!(*self)
    }

    /// Deserializes a CPU mask from a byte slice.
    pub fn from_slice(bytes: &[u8]) -> Self {
        let mut buf = [0u8; 128];
        let len = bytes.len().min(128);
        buf[..len].copy_from_slice(&bytes[..len]);
        Self::read_from_bytes(&buf).unwrap_or_default()
    }
}
