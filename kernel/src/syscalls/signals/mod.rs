use super::{SyscallError, SyscallResult, UserPtr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::ipc::signal::{is_uncatchable, SigAction, SigSet};

// ── Modular syscall submodules ──────────────────────────────────────────
pub mod kill;
pub mod rt_sigaction;
pub mod rt_sigprocmask;
pub mod rt_sigreturn;

pub use kill::sys_kill;
pub use rt_sigaction::sys_rt_sigaction;
pub use rt_sigprocmask::sys_rt_sigprocmask;
pub use rt_sigreturn::sys_rt_sigreturn;

