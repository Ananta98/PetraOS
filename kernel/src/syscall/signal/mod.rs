/// Signal-related system call handlers.
///
/// Implements the following Linux x86-64 ABI syscalls:
///
/// | Number | Name               | File             |
/// |--------|--------------------|------------------|
/// | 13     | rt_sigaction       | sigaction.rs     |
/// | 14     | rt_sigprocmask     | sigprocmask.rs   |
/// | 15     | rt_sigreturn       | sigreturn.rs     |
/// | 34     | rt_sigpending      | sigpending.rs    |
/// | 62     | kill               | kill.rs          |
/// | 72     | rt_sigsuspend      | sigsuspend.rs    |
/// | 234    | tgkill             | tgkill.rs        |
pub(crate) mod kill;
pub(crate) mod sigaction;
pub(crate) mod sigpending;
pub(crate) mod sigprocmask;
pub(crate) mod sigreturn;
pub(crate) mod sigsuspend;
pub(crate) mod tgkill;

pub(crate) use kill::syscall_kill;
pub(crate) use sigaction::syscall_rt_sigaction;
pub(crate) use sigpending::syscall_rt_sigpending;
pub(crate) use sigprocmask::syscall_rt_sigprocmask;
pub(crate) use sigreturn::syscall_rt_sigreturn;
pub(crate) use sigsuspend::syscall_rt_sigsuspend;
pub(crate) use tgkill::syscall_tgkill;
