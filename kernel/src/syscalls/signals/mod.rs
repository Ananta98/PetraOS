
// ── Modular syscall submodules ──────────────────────────────────────────
pub mod kill;
pub mod rt_sigaction;
pub mod rt_sigprocmask;
pub mod rt_sigreturn;

pub use kill::sys_kill;
pub use rt_sigaction::sys_rt_sigaction;
pub use rt_sigprocmask::sys_rt_sigprocmask;
pub use rt_sigreturn::sys_rt_sigreturn;

