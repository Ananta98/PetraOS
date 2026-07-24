//! SoftIRQ — Deferred / Bottom-Half Interrupt Processing for PetraOS.

use alloc::sync::Arc;
use core::sync::atomic::{AtomicU32, Ordering};
use ostd::sync::SpinLock;

/// Numeric identifiers for each SoftIRQ vector (lower = higher priority).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum SoftIrqVector {
    /// High-priority tasklet execution (HI_SOFTIRQ).
    Hi = 0,
    /// Timer expiry processing (TIMER_SOFTIRQ).
    Timer = 1,
    /// Network transmit completion (NET_TX_SOFTIRQ).
    NetTx = 2,
    /// Network receive processing (NET_RX_SOFTIRQ).
    NetRx = 3,
    /// Block I/O completion (BLOCK_SOFTIRQ).
    Block = 4,
    /// Standard deferred tasklet execution (TASKLET_SOFTIRQ).
    Tasklet = 5,
    /// Scheduler rebalancing / load-balance (SCHED_SOFTIRQ).
    Sched = 6,
}

impl SoftIrqVector {
    /// Total number of defined softirq vectors.
    pub const COUNT: usize = 7;

    /// Convert a raw `u8` index to a [`SoftIrqVector`], returning `None` if
    /// the index is out of range.
    pub fn from_u8(index: u8) -> Option<Self> {
        match index {
            0 => Some(Self::Hi),
            1 => Some(Self::Timer),
            2 => Some(Self::NetTx),
            3 => Some(Self::NetRx),
            4 => Some(Self::Block),
            5 => Some(Self::Tasklet),
            6 => Some(Self::Sched),
            _ => None,
        }
    }
}

/// Bottom-half callback type registered per vector.
pub type SoftIrqAction = Arc<dyn Fn() + Send + Sync + 'static>;

/// Per-vector registered bottom-half handlers.
static ACTIONS: SpinLock<[Option<SoftIrqAction>; SoftIrqVector::COUNT]> =
    SpinLock::new([None, None, None, None, None, None, None]);

/// Bitmask of vectors currently pending execution.
static PENDING_MASK: AtomicU32 = AtomicU32::new(0);

/// Register a bottom-half handler for `vec`.
pub fn open_softirq(vec: SoftIrqVector, handler: impl Fn() + Send + Sync + 'static) {
    let mut table = ACTIONS.lock();
    table[vec as usize] = Some(Arc::new(handler));
}

/// Raise `vec`, marking it as pending.
pub fn raise_softirq(vec: SoftIrqVector) {
    PENDING_MASK.fetch_or(1 << (vec as u8), Ordering::Release);
}

/// Return `true` if at least one vector is pending.
#[inline]
pub fn softirq_pending() -> bool {
    PENDING_MASK.load(Ordering::Acquire) != 0
}

/// Process all pending softirq vectors in priority order.
pub fn do_softirq() {
    let pending = PENDING_MASK.swap(0, Ordering::AcqRel);
    if pending == 0 {
        return;
    }

    let snapshot: [Option<SoftIrqAction>; SoftIrqVector::COUNT] = {
        let table = ACTIONS.lock();
        table.clone()
    };

    for (bit, action) in snapshot.iter().enumerate() {
        if (pending & (1 << bit)) != 0 {
            if let Some(handler) = action {
                handler();
            }
        }
    }
}
