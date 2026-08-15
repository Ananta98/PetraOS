use core::fmt;
use core::sync::atomic::{AtomicU64, Ordering};

/// Global atomic counter for auto-generating unique Process IDs.
static NEXT_PID: AtomicU64 = AtomicU64::new(1);

/// Unique process identifier (PID).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProcessId(pub u64);

impl ProcessId {
    /// Creates a `ProcessId` from a specific raw `u64` value.
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// Allocates and returns the next unique `ProcessId` using atomic increment.
    pub fn next() -> Self {
        Self(NEXT_PID.fetch_add(1, Ordering::Relaxed))
    }

    /// Returns the underlying raw `u64` identifier.
    pub const fn as_u64(&self) -> u64 {
        self.0
    }
}

impl From<u64> for ProcessId {
    fn from(id: u64) -> Self {
        Self(id)
    }
}

impl From<ProcessId> for u64 {
    fn from(pid: ProcessId) -> Self {
        pid.0
    }
}

impl fmt::Display for ProcessId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PID({})", self.0)
    }
}

/// Helper function for backward compatibility to generate the next `ProcessId`.
pub fn next_pid() -> ProcessId {
    ProcessId::next()
}
