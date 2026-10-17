// SPDX-License-Identifier: GPL-2.0

//! Process and Thread identifier types and high-performance bitmap allocator.

use core::num::NonZeroU32;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::api::errno::Result;

/// Maximum process identifier value.
pub const PID_MAX: u32 = 99999;

/// Fixed PID for the init process.
pub const PID_INIT: Pid = Pid(NonZeroU32::MIN);

/// Maximum thread identifier value.
pub const TID_MAX: u32 = 99999;

/// Unique process identifier wrapper.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pid(pub NonZeroU32);

impl Pid {
    pub const fn from_u32(val: u32) -> Option<Self> {
        match NonZeroU32::new(val) {
            Some(nz) => Some(Self(nz)),
            None => None,
        }
    }

    pub const fn as_u32(&self) -> u32 {
        self.0.get()
    }
}

impl core::fmt::Display for Pid {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.as_u32())
    }
}

/// Unique thread identifier wrapper.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Tid(pub NonZeroU32);

impl Tid {
    pub const fn from_u32(val: u32) -> Option<Self> {
        match NonZeroU32::new(val) {
            Some(nz) => Some(Self(nz)),
            None => None,
        }
    }

    pub const fn as_u32(&self) -> u32 {
        self.0.get()
    }
}

impl core::fmt::Display for Tid {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.as_u32())
    }
}

static NEXT_TID: AtomicU32 = AtomicU32::new(1);

/// Allocates a new thread identifier monotonically.
pub fn alloc_tid() -> Tid {
    let raw = NEXT_TID.fetch_add(1, Ordering::Relaxed);
    let raw = if raw > TID_MAX {
        NEXT_TID.store(2, Ordering::Relaxed);
        1
    } else {
        raw
    };
    Tid::from_u32(raw).unwrap_or(Tid(NonZeroU32::MIN))
}

const BITMAP_WORDS: usize = (PID_MAX as usize + 63) / 64;

/// High-performance BitSet PID allocator covering [0..=PID_MAX].
pub struct PidAllocator {
    next: u32,
    words: [u64; BITMAP_WORDS],
}

impl PidAllocator {
    pub const FIRST_ALLOCATABLE: u32 = 2;

    pub const fn new() -> Self {
        let mut words = [0u64; BITMAP_WORDS];
        // Reserve PID 0 (invalid) and PID 1 (init)
        words[0] = 0b11;
        Self {
            next: Self::FIRST_ALLOCATABLE,
            words,
        }
    }

    #[inline]
    fn is_set(&self, pid: u32) -> bool {
        if pid > PID_MAX {
            return true;
        }
        let word_idx = (pid / 64) as usize;
        let bit_idx = pid % 64;
        (self.words[word_idx] & (1u64 << bit_idx)) != 0
    }

    #[inline]
    fn set_bit(&mut self, pid: u32) {
        if pid <= PID_MAX {
            let word_idx = (pid / 64) as usize;
            let bit_idx = pid % 64;
            self.words[word_idx] |= 1u64 << bit_idx;
        }
    }

    #[inline]
    fn clear_bit(&mut self, pid: u32) {
        if pid <= PID_MAX {
            let word_idx = (pid / 64) as usize;
            let bit_idx = pid % 64;
            self.words[word_idx] &= !(1u64 << bit_idx);
        }
    }

    /// Allocates the next available PID in O(1) amortized bitwise scan.
    pub fn allocate(&mut self) -> Result<Pid> {
        let start = self.next;
        loop {
            let candidate = self.next;
            self.next = if self.next >= PID_MAX {
                Self::FIRST_ALLOCATABLE
            } else {
                self.next + 1
            };

            if !self.is_set(candidate) {
                self.set_bit(candidate);
                let pid = Pid::from_u32(candidate).expect("candidate is >= 1");
                return Ok(pid);
            }

            if self.next == start {
                crate::return_errno!(EAGAIN, "no available process IDs");
            }
        }
    }

    /// Frees an allocated PID.
    pub fn free(&mut self, pid: Pid) {
        let val = pid.as_u32();
        if val >= Self::FIRST_ALLOCATABLE {
            self.clear_bit(val);
        }
    }

    /// Reserves a specific PID (e.g. PID 1 for init).
    pub fn reserve(&mut self, pid: Pid) -> Result<()> {
        let val = pid.as_u32();
        if val > PID_MAX {
            crate::return_errno!(EINVAL, "PID exceeds PID_MAX");
        }
        self.set_bit(val);
        Ok(())
    }
}
