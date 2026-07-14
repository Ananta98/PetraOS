/// Inter-Process Communication — Signal Subsystem
///
/// Provides the POSIX-compatible signal IPC mechanism for PetraOS.
///
/// # Module layout
///
/// - [`types`]    — Signal numbers, `SigSet`, `SigAction`, `SigHandler`,
///                  `SigInfo`, and default-action table.
/// - [`queue`]    — Per-process pending signal queue (standard + RT queues,
///                  blocked-mask management).
/// - [`table`]    — Per-process installed signal action table.
/// - [`dispatch`] — Signal dispatch engine called at kernel-to-user transitions.
pub mod dispatch;
pub mod queue;
pub mod table;
pub mod types;

pub use dispatch::{DispatchOutcome, dispatch_pending, send_signal_to_group, send_signal_to_pid};
pub use queue::SigQueue;
pub use table::{SigHandlerKind, SigTable};
pub use types::{
    DefaultAction, SigAction, SigHandler, SigInfo, SigSet, default_action,
    SIGABRT, SIGALRM, SIGBUS, SIGCHLD, SIGCONT, SIGFPE, SIGHUP, SIGILL,
    SIGINT, SIGKILL, SIGPIPE, SIGPWR, SIGPROF, SIGQUIT, SIGRTMAX, SIGRTMIN,
    SIGSEGV, SIGSTOP, SIGSYS, SIGTERM, SIGTRAP, SIGTSTP, SIGTTIN, SIGTTOU,
    SIGUSR1, SIGUSR2, SIGVTALRM, SIGWINCH, SIGXCPU, SIGXFSZ,
};

// ──────────────────────────────────────────────────────────────
// ProcessSignals bundle
// ──────────────────────────────────────────────────────────────

/// Combined signal state for one process.
///
/// Holds both the pending signal queue and the installed action table.
/// Every [`Process`][crate::proc::process::Process] owns one instance of this
/// type, accessible through [`Process::signals`].
///
/// # Why a separate struct?
///
/// Bundling both fields together avoids the need to take two separate locks
/// (one for the queue and one for the table) in the hot dispatch path.
/// The caller can access both fields on the same borrow without risking a
/// deadlock between two different `SpinLock`s.
pub struct ProcessSignals {
    /// Pending signals waiting to be delivered.
    pub queue: SigQueue,
    /// Installed signal action table.
    pub table: SigTable,
}

impl ProcessSignals {
    /// Create an empty signal state (no pending signals, all default actions).
    pub const fn new() -> Self {
        Self {
            queue: SigQueue::new(),
            table: SigTable::new(),
        }
    }

    /// Reset signal dispositions on `exec`.
    ///
    /// POSIX requires that all signals set to a user handler are reset to
    /// `SIG_DFL` across `execve`.  Signals set to `SIG_IGN` are preserved.
    /// The pending queue is not cleared — pending signals survive exec.
    pub fn reset_on_exec(&self) {
        self.table.reset_on_exec();
    }
}

// ──────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;

    // ── SigSet tests ──────────────────────────────────────────────────────

    #[ktest]
    fn test_sigset_basic_operations() {
        let mut set = SigSet::EMPTY;
        assert!(set.is_empty());

        set.add(SIGTERM);
        assert!(set.contains(SIGTERM));
        assert!(!set.contains(SIGKILL));

        set.add(SIGKILL);
        assert!(set.contains(SIGKILL));
        assert_eq!(set.lowest(), Some(SIGKILL)); // SIGKILL = 9, SIGTERM = 15

        set.remove(SIGKILL);
        assert!(!set.contains(SIGKILL));
        assert_eq!(set.lowest(), Some(SIGTERM));

        set.remove(SIGTERM);
        assert!(set.is_empty());
    }

    #[ktest]
    fn test_sigset_union_intersection_complement() {
        let mut a = SigSet::EMPTY;
        a.add(SIGINT);
        a.add(SIGTERM);

        let mut b = SigSet::EMPTY;
        b.add(SIGTERM);
        b.add(SIGHUP);

        let union = a.union(b);
        assert!(union.contains(SIGINT));
        assert!(union.contains(SIGTERM));
        assert!(union.contains(SIGHUP));

        let inter = a.intersection(b);
        assert!(!inter.contains(SIGINT));
        assert!(inter.contains(SIGTERM));
        assert!(!inter.contains(SIGHUP));

        let comp = a.complement();
        assert!(!comp.contains(SIGINT));
        assert!(!comp.contains(SIGTERM));
        assert!(comp.contains(SIGHUP));
    }

    // ── SigQueue tests ────────────────────────────────────────────────────

    #[ktest]
    fn test_sigqueue_standard_signal_dedup() {
        let queue = SigQueue::new();

        // Sending the same standard signal twice should only produce one delivery.
        queue.enqueue(SigInfo::user(SIGTERM, 1));
        queue.enqueue(SigInfo::user(SIGTERM, 2));

        let first = queue.dequeue();
        assert!(first.is_some());
        assert_eq!(first.unwrap().signum, SIGTERM);

        // Second dequeue should return None (deduplicated).
        assert!(queue.dequeue().is_none());
    }

    #[ktest]
    fn test_sigqueue_priority_order() {
        let queue = SigQueue::new();
        // Enqueue high-numbered signal first, then lower.
        queue.enqueue(SigInfo::user(SIGTERM, 1)); // signum 15
        queue.enqueue(SigInfo::user(SIGINT, 1)); // signum 2

        // Lowest signal number should be dequeued first.
        let first = queue.dequeue().unwrap();
        assert_eq!(first.signum, SIGINT);
        let second = queue.dequeue().unwrap();
        assert_eq!(second.signum, SIGTERM);
    }

    #[ktest]
    fn test_sigqueue_blocking() {
        let queue = SigQueue::new();

        // Block SIGTERM before enqueueing.
        let mut blocked = SigSet::EMPTY;
        blocked.add(SIGTERM);
        queue.set_mask(blocked);

        queue.enqueue(SigInfo::user(SIGTERM, 1));
        queue.enqueue(SigInfo::user(SIGINT, 1));

        // SIGTERM is blocked; only SIGINT should be deliverable.
        let first = queue.dequeue().unwrap();
        assert_eq!(first.signum, SIGINT);

        // SIGTERM still pending but blocked.
        assert!(queue.dequeue().is_none());

        // Unblock SIGTERM.
        queue.unblock(blocked);
        let second = queue.dequeue().unwrap();
        assert_eq!(second.signum, SIGTERM);
    }

    #[ktest]
    fn test_sigqueue_sigkill_not_blockable() {
        let queue = SigQueue::new();

        // Attempt to block SIGKILL.
        let mut blocked = SigSet::EMPTY;
        blocked.add(SIGKILL);
        queue.set_mask(blocked);

        // SIGKILL must still be deliverable.
        queue.enqueue(SigInfo::kernel(SIGKILL));
        let result = queue.dequeue();
        assert!(result.is_some());
        assert_eq!(result.unwrap().signum, SIGKILL);
    }

    // ── SigTable tests ────────────────────────────────────────────────────

    #[ktest]
    fn test_sigtable_default_and_ignore() {
        let table = SigTable::new();

        // Fresh table: all signals default.
        assert!(matches!(
            table.get_handler_kind(SIGTERM),
            SigHandlerKind::Default
        ));

        // Install ignore.
        table.set_action(SIGTERM, SigAction::ignore());
        assert!(matches!(
            table.get_handler_kind(SIGTERM),
            SigHandlerKind::Ignore
        ));
    }

    #[ktest]
    fn test_sigtable_user_handler() {
        let table = SigTable::new();
        table.set_action(SIGUSR1, SigAction::user_handler(0xDEAD_BEEF, SigSet::EMPTY, 0));
        assert!(matches!(
            table.get_handler_kind(SIGUSR1),
            SigHandlerKind::UserHandler(0xDEAD_BEEF)
        ));
    }

    #[ktest]
    fn test_sigtable_sigkill_cannot_be_changed() {
        let table = SigTable::new();
        let result = table.set_action(SIGKILL, SigAction::ignore());
        assert!(result.is_none());
        // Should still be Default.
        assert!(matches!(
            table.get_handler_kind(SIGKILL),
            SigHandlerKind::Default
        ));
    }

    #[ktest]
    fn test_sigtable_reset_on_exec() {
        let table = SigTable::new();
        // Set a user handler and an ignore disposition.
        table.set_action(SIGUSR1, SigAction::user_handler(0x1000, SigSet::EMPTY, 0));
        table.set_action(SIGTERM, SigAction::ignore());

        table.reset_on_exec();

        // User handler must revert to default.
        assert!(matches!(
            table.get_handler_kind(SIGUSR1),
            SigHandlerKind::Default
        ));
        // Ignore is preserved across exec.
        assert!(matches!(
            table.get_handler_kind(SIGTERM),
            SigHandlerKind::Ignore
        ));
    }

    // ── ProcessSignals bundle ─────────────────────────────────────────────

    #[ktest]
    fn test_process_signals_send_and_dispatch() {
        use crate::proc::process::Process;
        use crate::vm::VMA_MANAGER;

        crate::vm::init();
        let vm = VMA_MANAGER.get().unwrap().clone();
        let mut process = Process::new(vm, "sig-test");

        // Install an ignore handler for SIGHUP.
        process.signals.table.set_action(SIGHUP, SigAction::ignore());

        // Send SIGHUP and SIGTERM.
        process.signals.queue.enqueue(SigInfo::user(SIGHUP, 0));
        process.signals.queue.enqueue(SigInfo::user(SIGTERM, 0));

        // Dispatch: SIGHUP should be ignored, SIGTERM's default action terminates.
        let outcome = dispatch_pending(&mut process);
        assert!(matches!(outcome, DispatchOutcome::Terminated { signum: 15 }));
    }

    #[ktest]
    fn test_process_signals_user_handler() {
        use crate::proc::process::Process;
        use crate::vm::VMA_MANAGER;

        crate::vm::init();
        let vm = VMA_MANAGER.get().unwrap().clone();
        let mut process = Process::new(vm, "user-handler-test");

        // Install a fake user-space handler at address 0x4000.
        process
            .signals
            .table
            .set_action(SIGUSR1, SigAction::user_handler(0x4000, SigSet::EMPTY, 0));

        process.signals.queue.enqueue(SigInfo::user(SIGUSR1, 42));

        let outcome = dispatch_pending(&mut process);
        assert!(matches!(
            outcome,
            DispatchOutcome::PendingUserHandler {
                signum: 10,
                handler_address: 0x4000,
            }
        ));
    }
}
