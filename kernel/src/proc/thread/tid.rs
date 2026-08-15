use core::fmt;
use core::sync::atomic::{AtomicU64, Ordering};

/// Global atomic counter for auto-generating unique Thread IDs.
static NEXT_TID: AtomicU64 = AtomicU64::new(1);

/// Opaque, unique identifier for a thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ThreadId(pub u64);

impl ThreadId {
    /// Creates a `ThreadId` from a specific raw `u64` value.
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// Allocates and returns the next unique `ThreadId` using atomic increment.
    pub fn next() -> Self {
        Self(NEXT_TID.fetch_add(1, Ordering::Relaxed))
    }

    /// Returns the underlying raw `u64` identifier.
    pub const fn as_u64(&self) -> u64 {
        self.0
    }
}

impl From<u64> for ThreadId {
    fn from(id: u64) -> Self {
        Self(id)
    }
}

impl From<ThreadId> for u64 {
    fn from(tid: ThreadId) -> Self {
        tid.0
    }
}

impl fmt::Display for ThreadId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TID({})", self.0)
    }
}

/// Helper function to generate the next `ThreadId`.
pub fn next_tid() -> ThreadId {
    ThreadId::next()
}
