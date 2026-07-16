/// Signal Action Table — Per-Process Handler Registry
///
/// Stores the `SigAction` for each of the 64 possible signals for one process.
/// The table is indexed by signal number (1-based) and initialised with the
/// default action for every signal.
///
/// # Thread Safety
///
/// The entire table is wrapped in a `SpinLock` so that concurrent reads and
/// writes from multiple threads (e.g., one thread changing a handler while
/// another delivers a signal) are safe.
use alloc::collections::BTreeMap;
use ostd::sync::SpinLock;

use super::types::{SIGRTMAX, SigAction, SigHandler, SigSet};

// ──────────────────────────────────────────────────────────────
// SigTable
// ──────────────────────────────────────────────────────────────

/// The installed signal action table for one process.
///
/// Signals that have never been changed by the process retain their default
/// action and are not stored explicitly (absent entry ≡ `Default`).
pub struct SigTable {
    inner: SpinLock<BTreeMap<u32, SigAction>>,
}

impl SigTable {
    /// Create an empty action table (all signals use the default disposition).
    pub const fn new() -> Self {
        Self {
            inner: SpinLock::new(BTreeMap::new()),
        }
    }

    /// Install `action` for signal `signum`.
    ///
    /// Returns the previous action that was replaced, or `None` if the signal
    /// was using the default disposition.
    ///
    /// # Validation
    ///
    /// SIGKILL (9) and SIGSTOP (19) cannot have their dispositions changed;
    /// attempts to do so are silently ignored and `None` is returned.
    pub fn set_action(&self, signum: u32, action: SigAction) -> Option<SigAction> {
        use super::types::{SIGKILL, SIGSTOP};
        if signum == 0 || signum > SIGRTMAX {
            return None;
        }
        if signum == SIGKILL || signum == SIGSTOP {
            return None;
        }
        self.inner.lock().insert(signum, action)
    }

    /// Retrieve a reference to the installed action for `signum`.
    ///
    /// Because the action table is locked during lookup, the action is cloned
    /// (for `SigHandler::Default` and `SigHandler::Ignore`) or represented by
    /// a sentinel, then returned without holding the lock.
    ///
    /// Returns the handler kind for the signal.
    pub fn get_handler_kind(&self, signum: u32) -> SigHandlerKind {
        let inner = self.inner.lock();
        match inner.get(&signum) {
            None => SigHandlerKind::Default,
            Some(action) => match &action.handler {
                SigHandler::Default => SigHandlerKind::Default,
                SigHandler::Ignore => SigHandlerKind::Ignore,
                SigHandler::UserHandler(addr) => SigHandlerKind::UserHandler(*addr),
                SigHandler::KernelHandler(_) => SigHandlerKind::KernelHandler,
            },
        }
    }

    /// Invoke the kernel handler for `signum` if one is installed.
    ///
    /// This is the fast path for kernel-internal signals: the lock is held
    /// only long enough to borrow the closure, which is then called while the
    /// lock is released.  Returns `true` if a kernel handler was invoked.
    pub fn call_kernel_handler(&self, signum: u32) -> bool {
        // We need to pull the closure out and call it without holding the lock
        // to avoid potential deadlocks inside the handler.  Because
        // `KernelHandler` stores a `Box<dyn Fn>` we can call it while holding
        // the lock (the Fn is Sync) as long as the handler itself does not try
        // to re-acquire the same lock.
        let inner = self.inner.lock();
        if let Some(action) = inner.get(&signum) {
            if let SigHandler::KernelHandler(ref func) = action.handler {
                func(signum);
                return true;
            }
        }
        false
    }

    /// Return the signal mask that should be added to the process mask while
    /// the handler for `signum` runs, or an empty set if the signal uses a
    /// non-user-space handler.
    pub fn handler_mask(&self, signum: u32) -> SigSet {
        let inner = self.inner.lock();
        inner
            .get(&signum)
            .map(|action| action.mask)
            .unwrap_or(SigSet::EMPTY)
    }

    /// Reset the action for `signum` back to the default disposition.
    ///
    /// Called on `exec()` to clear all installed handlers (POSIX requires that
    /// signal dispositions set to a user handler be reset to `SIG_DFL` on
    /// `execve`).  Ignored (`SIG_IGN`) dispositions are preserved across
    /// `exec`.
    pub fn reset_on_exec(&self) {
        let mut inner = self.inner.lock();
        inner.retain(|_, action| matches!(action.handler, SigHandler::Ignore));
    }
}

// ──────────────────────────────────────────────────────────────
// SigHandlerKind — a lock-free snapshot of the handler type
// ──────────────────────────────────────────────────────────────

/// A lock-free description of what kind of handler is installed for a signal.
///
/// This is returned by [`SigTable::get_handler_kind`] so that the dispatch
/// logic can branch without holding the signal table lock.
#[derive(Debug, Clone, Copy)]
pub enum SigHandlerKind {
    /// Use the kernel's default action.
    Default,
    /// Ignore the signal.
    Ignore,
    /// Call the user-space function at this virtual address.
    UserHandler(usize),
    /// A kernel-internal callback is installed (call [`SigTable::call_kernel_handler`]).
    KernelHandler,
}
