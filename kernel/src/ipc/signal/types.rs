/// POSIX Signal Definitions
///
/// Provides the standard Unix signal numbers, the `SigSet` bitmask type for
/// representing sets of signals, and the `SigAction` / `SigHandler` types that
/// describe what the kernel should do when a signal is delivered to a process.
///
/// # Design
///
/// Signal numbers follow the POSIX / Linux x86-64 ABI exactly so that
/// user-space programs compiled for Linux run correctly under PetraOS without
/// any remapping.  Numbers are represented as `u32` to match the width of the
/// `sigset_t` word on a 32-bit word boundary; a `SigSet` wraps a single `u64`
/// bitmap covering signals 1 – 64.
use alloc::boxed::Box;

// ──────────────────────────────────────────────────────────────
// Signal numbers (POSIX + Linux supplementary)
// ──────────────────────────────────────────────────────────────

/// Hangup.  Sent when the controlling terminal is closed.
pub const SIGHUP: u32 = 1;
/// Interactive attention (^C).
pub const SIGINT: u32 = 2;
/// Quit (^\\).
pub const SIGQUIT: u32 = 3;
/// Illegal instruction.
pub const SIGILL: u32 = 4;
/// Trace / breakpoint trap.
pub const SIGTRAP: u32 = 5;
/// Abort — sent by `abort(3)`.
pub const SIGABRT: u32 = 6;
/// Bus error (bad memory access alignment).
pub const SIGBUS: u32 = 7;
/// Floating-point exception.
pub const SIGFPE: u32 = 8;
/// Kill — cannot be caught or ignored.
pub const SIGKILL: u32 = 9;
/// User-defined signal 1.
pub const SIGUSR1: u32 = 10;
/// Invalid memory reference (segmentation fault).
pub const SIGSEGV: u32 = 11;
/// User-defined signal 2.
pub const SIGUSR2: u32 = 12;
/// Broken pipe (write to pipe with no reader).
pub const SIGPIPE: u32 = 13;
/// Alarm clock (set by `alarm(2)`).
pub const SIGALRM: u32 = 14;
/// Termination — default disposition terminates the process.
pub const SIGTERM: u32 = 15;
/// Stack fault on coprocessor (unused on modern hardware).
pub const SIGSTKFLT: u32 = 16;
/// Child stopped or terminated.
pub const SIGCHLD: u32 = 17;
/// Continue if stopped.
pub const SIGCONT: u32 = 18;
/// Stop process — cannot be caught or ignored.
pub const SIGSTOP: u32 = 19;
/// Stop typed at terminal (^Z).
pub const SIGTSTP: u32 = 20;
/// Terminal input for background process.
pub const SIGTTIN: u32 = 21;
/// Terminal output for background process.
pub const SIGTTOU: u32 = 22;
/// Urgent condition on socket.
pub const SIGURG: u32 = 23;
/// CPU time limit exceeded.
pub const SIGXCPU: u32 = 24;
/// File size limit exceeded.
pub const SIGXFSZ: u32 = 25;
/// Virtual alarm clock.
pub const SIGVTALRM: u32 = 26;
/// Profiling timer expired.
pub const SIGPROF: u32 = 27;
/// Window resize signal.
pub const SIGWINCH: u32 = 28;
/// I/O now possible.
pub const SIGIO: u32 = 29;
/// Power failure.
pub const SIGPWR: u32 = 30;
/// Bad system call.
pub const SIGSYS: u32 = 31;

/// The number of standard signals (signals 1 – 31).
pub const STANDARD_SIGNAL_COUNT: u32 = 31;
/// The highest real-time signal number supported (RT signals 32 – 64).
pub const SIGRTMAX: u32 = 64;
/// The lowest real-time signal number.
pub const SIGRTMIN: u32 = 32;

// ──────────────────────────────────────────────────────────────
// SigSet — bitmask of signals
// ──────────────────────────────────────────────────────────────

/// A bitmask that can represent an arbitrary set of signals 1 – 64.
///
/// Bit `n - 1` corresponds to signal `n` (signal 1 = bit 0).
/// This mirrors the Linux `sigset_t` layout for a 64-bit kernel.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SigSet(u64);

impl SigSet {
    /// An empty signal set (no signals pending / blocked).
    pub const EMPTY: Self = Self(0);

    /// Create a `SigSet` with a single signal `signum` set.
    ///
    /// Returns `None` if `signum` is 0 or greater than 64.
    pub fn from_signal(signum: u32) -> Option<Self> {
        if signum == 0 || signum > 64 {
            return None;
        }
        Some(Self(1u64 << (signum - 1)))
    }

    /// Add `signum` to the set.
    pub fn add(&mut self, signum: u32) {
        if signum > 0 && signum <= 64 {
            self.0 |= 1u64 << (signum - 1);
        }
    }

    /// Remove `signum` from the set.
    pub fn remove(&mut self, signum: u32) {
        if signum > 0 && signum <= 64 {
            self.0 &= !(1u64 << (signum - 1));
        }
    }

    /// Test whether `signum` is a member of the set.
    pub fn contains(&self, signum: u32) -> bool {
        if signum == 0 || signum > 64 {
            return false;
        }
        (self.0 >> (signum - 1)) & 1 != 0
    }

    /// Return `true` if the set has no signals.
    pub fn is_empty(&self) -> bool {
        self.0 == 0
    }

    /// Compute the bitwise union of two signal sets.
    pub fn union(&self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Compute the bitwise intersection of two signal sets.
    pub fn intersection(&self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    /// Compute the set complement (all signals not in `self`).
    pub fn complement(&self) -> Self {
        Self(!self.0)
    }

    /// Return the lowest-numbered signal in the set, or `None` if empty.
    ///
    /// Used during signal delivery to pick the next pending signal.
    pub fn lowest(&self) -> Option<u32> {
        if self.0 == 0 {
            return None;
        }
        // trailing_zeros gives index of lowest set bit (0-based).
        let bit = self.0.trailing_zeros();
        Some(bit + 1)
    }

    /// Raw bitmask value.
    pub fn as_u64(&self) -> u64 {
        self.0
    }

    /// Construct from a raw 64-bit bitmask.
    pub fn from_u64(raw: u64) -> Self {
        Self(raw)
    }
}

// ──────────────────────────────────────────────────────────────
// Default signal dispositions
// ──────────────────────────────────────────────────────────────

/// The default kernel action for a signal when no handler has been installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultAction {
    /// Terminate the process (e.g., SIGTERM, SIGKILL).
    Terminate,
    /// Terminate the process and generate a core dump (e.g., SIGQUIT, SIGSEGV).
    CoreDump,
    /// Stop the process (e.g., SIGSTOP, SIGTSTP).
    Stop,
    /// Continue a stopped process (e.g., SIGCONT).
    Continue,
    /// Ignore the signal (e.g., SIGCHLD, SIGURG).
    Ignore,
}

/// Return the default kernel action for the given signal number.
///
/// Mirrors the POSIX-defined default dispositions documented in `signal(7)`.
pub fn default_action(signum: u32) -> DefaultAction {
    match signum {
        SIGHUP | SIGINT | SIGKILL | SIGPIPE | SIGALRM | SIGTERM | SIGUSR1 | SIGUSR2 | SIGSTKFLT
        | SIGPWR | SIGPROF | SIGVTALRM | SIGIO | SIGXCPU | SIGXFSZ => DefaultAction::Terminate,

        SIGQUIT | SIGILL | SIGABRT | SIGFPE | SIGSEGV | SIGBUS | SIGTRAP | SIGSYS => {
            DefaultAction::CoreDump
        }

        SIGSTOP | SIGTSTP | SIGTTIN | SIGTTOU => DefaultAction::Stop,

        SIGCONT => DefaultAction::Continue,

        // SIGCHLD, SIGURG, SIGWINCH → ignored by default.
        _ => DefaultAction::Ignore,
    }
}

// ──────────────────────────────────────────────────────────────
// SigHandler — callable user-space or kernel handler
// ──────────────────────────────────────────────────────────────

/// The handler installed for a specific signal.
///
/// This is a kernel-side representation; user-space virtual addresses are
/// stored as `usize` and invoked by the architecture-specific signal trampoline
/// (to be wired up by the syscall layer once `sys_sigaction` is implemented).
pub enum SigHandler {
    /// Use the kernel's default action for this signal.
    Default,
    /// Ignore this signal completely (equivalent to `SIG_IGN`).
    Ignore,
    /// User-space signal handler function at the given virtual address.
    ///
    /// The `usize` is the virtual address of the handler function in the
    /// process's address space.  Delivery is completed by the architecture
    /// signal trampoline once control returns to user mode.
    UserHandler(usize),
    /// Kernel-internal handler (used for SIGCHLD notification, etc.)
    ///
    /// Executed directly in kernel mode during signal dispatch.
    KernelHandler(Box<dyn Fn(u32) + Send + Sync>),
}

impl core::fmt::Debug for SigHandler {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Default => formatter.write_str("SigHandler::Default"),
            Self::Ignore => formatter.write_str("SigHandler::Ignore"),
            Self::UserHandler(addr) => {
                formatter.write_fmt(format_args!("SigHandler::UserHandler({:#x})", addr))
            }
            Self::KernelHandler(_) => formatter.write_str("SigHandler::KernelHandler(<fn>)"),
        }
    }
}

// ──────────────────────────────────────────────────────────────
// SigAction — complete signal action descriptor
// ──────────────────────────────────────────────────────────────

/// A complete description of how a signal should be handled.
///
/// This mirrors the POSIX `struct sigaction` but adapted for the kernel.
///
/// # Flags (future)
/// When the syscall layer adds `sys_sigaction`, it will populate `flags` from
/// the `sa_flags` field of the user-space `sigaction` struct.
#[derive(Debug)]
pub struct SigAction {
    /// The handler (default, ignore, user-space, or kernel callback).
    pub handler: SigHandler,
    /// Signals to block (add to the signal mask) while the handler runs.
    pub mask: SigSet,
    /// SA_* flags (e.g., `SA_RESTART`, `SA_SIGINFO`).  Zero for now.
    pub flags: u32,
}

impl SigAction {
    /// Construct a `SigAction` that ignores the signal.
    pub fn ignore() -> Self {
        Self {
            handler: SigHandler::Ignore,
            mask: SigSet::EMPTY,
            flags: 0,
        }
    }

    /// Construct a `SigAction` with a user-space handler.
    pub fn user_handler(handler_address: usize, mask: SigSet, flags: u32) -> Self {
        Self {
            handler: SigHandler::UserHandler(handler_address),
            mask,
            flags,
        }
    }
}

// ──────────────────────────────────────────────────────────────
// SigInfo — metadata accompanying a delivered signal
// ──────────────────────────────────────────────────────────────

/// Supplementary information about a signal delivery (mirrors `siginfo_t`).
///
/// The `code` field distinguishes the reason a signal was sent (e.g., sent by
/// a user, by the kernel, or by a timer).  Only the fields relevant to the
/// current kernel are stored; the syscall layer will expose the full
/// `siginfo_t` layout to user-space programs.
#[derive(Debug, Clone, Copy)]
pub struct SigInfo {
    /// Signal number.
    pub signum: u32,
    /// PID of the sending process (0 if sent by the kernel).
    pub sender_pid: u32,
    /// Signal code (SI_USER = 0, SI_KERNEL = 128, etc.).
    pub code: i32,
}

impl SigInfo {
    /// Create a `SigInfo` for a user-sent signal.
    pub fn user(signum: u32, sender_pid: u32) -> Self {
        Self {
            signum,
            sender_pid,
            code: 0, // SI_USER
        }
    }

    /// Create a `SigInfo` for a kernel-generated signal.
    pub fn kernel(signum: u32) -> Self {
        Self {
            signum,
            sender_pid: 0,
            code: 128, // SI_KERNEL
        }
    }
}
