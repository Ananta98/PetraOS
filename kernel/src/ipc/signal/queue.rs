/// Per-Process Pending Signal Queue
///
/// Maintains the set of pending signals for a process and provides the
/// bookkeeping needed to merge, dequeue, and block signals correctly.
///
/// # Standard vs. Real-Time signals
///
/// POSIX defines two classes of signals:
///
/// - **Standard signals (1–31):** Not queued — if the same standard signal is
///   sent multiple times before delivery, only one instance is remembered.
///   This is captured by storing pending standard signals as a `SigSet` bitmap.
///
/// - **Real-time signals (32–64):** Can be queued — multiple instances of the
///   same RT signal sent before delivery are all individually queued.  This is
///   implemented with a separate `VecDeque<SigInfo>` for RT signals.
///
/// # Signal Mask
///
/// The `blocked` mask holds the signals that are currently blocked (deferred)
/// for the process.  A signal in the `blocked` mask stays pending until it is
/// unblocked (the mask is modified by `sigprocmask`).
use alloc::collections::VecDeque;
use ostd::sync::SpinLock;

use super::types::{SIGKILL, SIGSTOP, SigInfo, SigSet};

// ──────────────────────────────────────────────────────────────
// SigQueue inner state
// ──────────────────────────────────────────────────────────────

struct SigQueueInner {
    /// Bitmap of pending standard signals (1 – 31).
    /// Each bit represents one pending signal; duplicates are collapsed.
    pending_standard: SigSet,

    /// Queue of pending real-time signal instances (32 – 64).
    /// Multiple instances of the same RT signal are individually recorded.
    pending_realtime: VecDeque<SigInfo>,

    /// Signals that are blocked (masked) for this process.
    ///
    /// SIGKILL and SIGSTOP can never be blocked and are silently cleared from
    /// this mask when it is written.
    blocked: SigSet,
}

impl SigQueueInner {
    const fn new() -> Self {
        Self {
            pending_standard: SigSet::EMPTY,
            pending_realtime: VecDeque::new(),
            blocked: SigSet::EMPTY,
        }
    }
}

// ──────────────────────────────────────────────────────────────
// SigQueue — public API
// ──────────────────────────────────────────────────────────────

/// The pending-signal queue for one process.
///
/// Thread-safe: all internal state is protected by a `SpinLock`.
pub struct SigQueue {
    inner: SpinLock<SigQueueInner>,
}

impl SigQueue {
    /// Create an empty signal queue with no pending or blocked signals.
    pub const fn new() -> Self {
        Self {
            inner: SpinLock::new(SigQueueInner::new()),
        }
    }

    // ──────────────────────────────────────────────────────────
    // Enqueue
    // ──────────────────────────────────────────────────────────

    /// Send signal `info` to this queue.
    ///
    /// - Standard signals (1–31): sets the corresponding bit in
    ///   `pending_standard` — further sends of the same signal before it is
    ///   delivered are silently collapsed.
    /// - Real-time signals (32–64): push `info` onto `pending_realtime`,
    ///   preserving each individual instance.
    ///
    /// SIGKILL and SIGSTOP bypass the blocked mask check and are always
    /// marked pending.
    pub fn enqueue(&self, info: SigInfo) {
        let mut inner = self.inner.lock();
        if info.signum == 0 || info.signum > 64 {
            return;
        }
        if info.signum <= 31 {
            inner.pending_standard.add(info.signum);
        } else {
            inner.pending_realtime.push_back(info);
        }
    }

    // ──────────────────────────────────────────────────────────
    // Dequeue
    // ──────────────────────────────────────────────────────────

    /// Dequeue and return the next deliverable signal, or `None` if there
    /// are no unblocked pending signals.
    ///
    /// Delivery priority: standard signals are checked before RT signals.
    /// Within each class the lowest signal number takes priority.
    ///
    /// A signal is considered deliverable if it is pending **and** not in the
    /// current blocked mask — with the exception of SIGKILL (9) and SIGSTOP
    /// (19), which are always deliverable regardless of the mask.
    pub fn dequeue(&self) -> Option<SigInfo> {
        let mut inner = self.inner.lock();
        let deliverable_std = inner
            .pending_standard
            .intersection(inner.blocked.complement())
            // Always allow SIGKILL and SIGSTOP through, even if "blocked"
            .union(
                inner
                    .pending_standard
                    .intersection(SigSet::from_signal(SIGKILL).unwrap_or(SigSet::EMPTY))
                    .union(
                        inner
                            .pending_standard
                            .intersection(SigSet::from_signal(SIGSTOP).unwrap_or(SigSet::EMPTY)),
                    ),
            );

        // Try to deliver a standard signal first.
        if let Some(signum) = deliverable_std.lowest() {
            inner.pending_standard.remove(signum);
            return Some(SigInfo::kernel(signum));
        }

        // Try to deliver the oldest queued RT signal that is not blocked.
        let blocked = inner.blocked;
        if let Some(pos) = inner.pending_realtime.iter().position(|info| {
            !blocked.contains(info.signum) || info.signum == SIGKILL || info.signum == SIGSTOP
        }) {
            return inner.pending_realtime.remove(pos);
        }

        None
    }

    // ──────────────────────────────────────────────────────────
    // Inspection
    // ──────────────────────────────────────────────────────────

    /// Return `true` if there is at least one unblocked pending signal.
    pub fn has_pending(&self) -> bool {
        let inner = self.inner.lock();
        let deliverable = inner
            .pending_standard
            .intersection(inner.blocked.complement())
            .union(
                inner
                    .pending_standard
                    .intersection(SigSet::from_signal(SIGKILL).unwrap_or(SigSet::EMPTY))
                    .union(
                        inner
                            .pending_standard
                            .intersection(SigSet::from_signal(SIGSTOP).unwrap_or(SigSet::EMPTY)),
                    ),
            );
        if !deliverable.is_empty() {
            return true;
        }
        let blocked = inner.blocked;
        inner.pending_realtime.iter().any(|info| {
            !blocked.contains(info.signum) || info.signum == SIGKILL || info.signum == SIGSTOP
        })
    }

    /// Return a snapshot of all pending signals as a `SigSet`.
    ///
    /// Real-time signals that have instances in the RT queue are OR'd into the
    /// returned set even though their individual instances are not collapsed.
    pub fn pending_snapshot(&self) -> SigSet {
        let inner = self.inner.lock();
        let mut set = inner.pending_standard;
        for info in &inner.pending_realtime {
            set.add(info.signum);
        }
        set
    }

    // ──────────────────────────────────────────────────────────
    // Signal mask (blocked set)
    // ──────────────────────────────────────────────────────────

    /// Replace the entire signal mask with `mask`.
    ///
    /// SIGKILL and SIGSTOP are silently removed from the new mask; they can
    /// never be blocked per POSIX.
    pub fn set_mask(&self, mut mask: SigSet) {
        mask.remove(SIGKILL);
        mask.remove(SIGSTOP);
        self.inner.lock().blocked = mask;
    }

    /// Return a snapshot of the current signal mask.
    pub fn get_mask(&self) -> SigSet {
        self.inner.lock().blocked
    }

    /// Add `mask` to the current blocked set (SIG_BLOCK).
    pub fn block(&self, mask: SigSet) {
        let mut inner = self.inner.lock();
        let mut new_blocked = inner.blocked.union(mask);
        new_blocked.remove(SIGKILL);
        new_blocked.remove(SIGSTOP);
        inner.blocked = new_blocked;
    }

    /// Remove `mask` from the current blocked set (SIG_UNBLOCK).
    pub fn unblock(&self, mask: SigSet) {
        let mut inner = self.inner.lock();
        inner.blocked = SigSet::from_u64(inner.blocked.as_u64() & !mask.as_u64());
    }
}
