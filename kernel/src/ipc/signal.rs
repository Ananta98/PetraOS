//! Inter-Process Communication: Signals
//! Defines signal numbers, actions, and sets.

pub const SIGHUP: u8 = 1;
pub const SIGINT: u8 = 2;
pub const SIGQUIT: u8 = 3;
pub const SIGILL: u8 = 4;
pub const SIGTRAP: u8 = 5;
pub const SIGABRT: u8 = 6;
pub const SIGBUS: u8 = 7;
pub const SIGFPE: u8 = 8;
pub const SIGKILL: u8 = 9;
pub const SIGUSR1: u8 = 10;
pub const SIGSEGV: u8 = 11;
pub const SIGUSR2: u8 = 12;
pub const SIGPIPE: u8 = 13;
pub const SIGALRM: u8 = 14;
pub const SIGTERM: u8 = 15;
pub const SIGSTKFLT: u8 = 16;
pub const SIGCHLD: u8 = 17;
pub const SIGCONT: u8 = 18;
pub const SIGSTOP: u8 = 19;
pub const SIGTSTP: u8 = 20;
pub const SIGTTIN: u8 = 21;
pub const SIGTTOU: u8 = 22;
pub const SIGURG: u8 = 23;
pub const SIGXCPU: u8 = 24;
pub const SIGXFSZ: u8 = 25;
pub const SIGVTALRM: u8 = 26;
pub const SIGPROF: u8 = 27;
pub const SIGWINCH: u8 = 28;
pub const SIGIO: u8 = 29;
pub const SIGPWR: u8 = 30;
pub const SIGSYS: u8 = 31;

pub const MAX_SIGNALS: usize = 64;

pub const SIG_BLOCK: i32 = 0;
pub const SIG_UNBLOCK: i32 = 1;
pub const SIG_SETMASK: i32 = 2;

/// Default signal action (e.g., terminate process, core dump, etc.)
pub const SIG_DFL: usize = 0;
/// Ignore signal
pub const SIG_IGN: usize = 1;

/// A set of signals, represented as a bitmask.
pub type SigSet = u64;

/// Defines the action to take when a signal is delivered.
#[derive(Debug, Clone, Copy)]
pub struct SigAction {
    /// Address of the signal handler, or SIG_DFL/SIG_IGN.
    pub handler: usize,
    /// Flags modifying the behaviour of the signal.
    pub flags: usize,
    /// Mask of signals to block while the handler executes.
    pub mask: SigSet,
    /// Address of the restorer function (sigreturn trampoline).
    pub restorer: usize,
}

impl Default for SigAction {
    fn default() -> Self {
        Self {
            handler: SIG_DFL,
            flags: 0,
            mask: 0,
            restorer: 0,
        }
    }
}

/// Default signal action categories per POSIX.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalDefaultAction {
    Terminate,
    CoreDump,
    Ignore,
    Stop,
    Continue,
}

/// Returns the POSIX default action for a given signal number.
pub fn default_action(sig: u8) -> SignalDefaultAction {
    match sig {
        SIGCHLD | SIGURG | SIGWINCH => SignalDefaultAction::Ignore,
        SIGSTOP | SIGTSTP | SIGTTIN | SIGTTOU => SignalDefaultAction::Stop,
        SIGCONT => SignalDefaultAction::Continue,
        SIGQUIT | SIGILL | SIGTRAP | SIGABRT | SIGBUS | SIGFPE | SIGSEGV | SIGXCPU | SIGXFSZ
        | SIGSYS => SignalDefaultAction::CoreDump,
        _ => SignalDefaultAction::Terminate,
    }
}

/// Returns true if the signal cannot be caught, ignored, or blocked (SIGKILL & SIGSTOP).
pub fn is_uncatchable(sig: u8) -> bool {
    sig == SIGKILL || sig == SIGSTOP
}

/// Tracks pending signals for a thread or process.
#[derive(Debug, Default, Clone)]
pub struct PendingSignals {
    /// Mask of currently pending signals.
    pub mask: SigSet,
}

impl PendingSignals {
    pub fn new() -> Self {
        Self { mask: 0 }
    }

    /// Mark a signal as pending.
    pub fn add(&mut self, sig: u8) {
        if sig > 0 && sig <= 64 {
            self.mask |= 1 << (sig - 1);
        }
    }

    /// Check if a signal is pending.
    pub fn has(&self, sig: u8) -> bool {
        if sig > 0 && sig <= 64 {
            (self.mask & (1 << (sig - 1))) != 0
        } else {
            false
        }
    }

    /// Clear a pending signal.
    pub fn clear(&mut self, sig: u8) {
        if sig > 0 && sig <= 64 {
            self.mask &= !(1 << (sig - 1));
        }
    }

    /// Dequeue the lowest unblocked pending signal.
    /// Uncatchable signals (SIGKILL, SIGSTOP) are delivered even if in blocked_mask.
    pub fn dequeue(&mut self, blocked_mask: SigSet) -> Option<u8> {
        let unblocked = self.mask & (!blocked_mask | (1 << (SIGKILL - 1)) | (1 << (SIGSTOP - 1)));
        if unblocked == 0 {
            return None;
        }
        let sig_index = unblocked.trailing_zeros() as u8;
        let sig = sig_index + 1;
        self.clear(sig);
        Some(sig)
    }
}

/// Send a signal to all processes in a specified process group.
pub fn send_signal_to_process_group(pgid: i32, sig: u8) -> Result<(), ()> {
    if pgid <= 0 || sig == 0 || sig > 64 {
        return Err(());
    }
    let target_pgid = crate::proc::ProcessId(pgid as u64);
    let procs = crate::proc::find_processes_by_pgid(target_pgid);
    if procs.is_empty() {
        return Err(());
    }
    for proc_arc in procs {
        let mut proc = proc_arc.lock();
        let _ = proc.send_signal(sig);
    }
    Ok(())
}

