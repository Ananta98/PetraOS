//! Thread scheduling policies and priority definitions.
//!
//! Supports POSIX-compatible scheduling policies:
//! - `SCHED_OTHER` / `SCHED_NORMAL` (EEVDF fair scheduling)
//! - `SCHED_FIFO` (Fixed-priority real-time)
//! - `SCHED_RR` (Round-robin real-time with time slice)

/// Minimum valid real-time priority.
pub const MIN_RT_PRIO: u8 = 0;

/// Maximum valid real-time priority.
pub const MAX_RT_PRIO: u8 = 99;

/// Number of distinct real-time priority levels (0..=99).
pub const RT_PRIO_COUNT: usize = 100;

/// Default time quantum for `SCHED_RR` threads (100 ms in nanoseconds).
pub const DEFAULT_RR_QUANTUM_NS: u64 = 100_000_000;

/// Scheduling policies matching POSIX definitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum SchedPolicy {
    /// Normal time-sharing policy governed by EEVDF (POSIX `SCHED_OTHER`).
    Fair = 0,
    /// First-in, first-out real-time policy without time slice preemption (POSIX `SCHED_FIFO`).
    Fifo = 1,
    /// Round-robin real-time policy with preemption on quantum expiration (POSIX `SCHED_RR`).
    RoundRobin = 2,
}

impl SchedPolicy {
    /// Returns `true` if the policy is a real-time scheduling class (`Fifo` or `RoundRobin`).
    pub const fn is_realtime(&self) -> bool {
        matches!(self, Self::Fifo | Self::RoundRobin)
    }
}

impl Default for SchedPolicy {
    fn default() -> Self {
        Self::Fair
    }
}

/// A validated real-time priority value constrained to `[0, 99]`.
///
/// Higher numerical values represent higher scheduling priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct RtPriority(u8);

impl RtPriority {
    /// Minimum priority (0).
    pub const MIN: Self = Self(MIN_RT_PRIO);

    /// Maximum priority (99).
    pub const MAX: Self = Self(MAX_RT_PRIO);

    /// Default priority (0).
    pub const DEFAULT: Self = Self(MIN_RT_PRIO);

    /// Creates a new `RtPriority`, validating that `val <= 99`.
    pub const fn new(val: u8) -> Result<Self, &'static str> {
        if val <= MAX_RT_PRIO {
            Ok(Self(val))
        } else {
            Err("Real-time priority must be between 0 and 99")
        }
    }

    /// Creates an `RtPriority` by clamping to the `[0, 99]` range.
    pub const fn from_raw_clamped(val: u8) -> Self {
        if val > MAX_RT_PRIO {
            Self(MAX_RT_PRIO)
        } else {
            Self(val)
        }
    }

    /// Returns the raw numerical priority `[0, 99]`.
    pub const fn value(&self) -> u8 {
        self.0
    }
}

impl Default for RtPriority {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl From<RtPriority> for u8 {
    fn from(prio: RtPriority) -> Self {
        prio.0
    }
}

impl TryFrom<u8> for RtPriority {
    type Error = &'static str;

    fn try_from(val: u8) -> Result<Self, Self::Error> {
        Self::new(val)
    }
}
