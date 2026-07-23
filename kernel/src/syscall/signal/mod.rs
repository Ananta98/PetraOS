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
pub mod kill;
pub mod sigaction;
pub mod sigpending;
pub mod sigprocmask;
pub mod sigqueueinfo;
pub mod sigreturn;
pub mod sigsuspend;
pub mod sigtimedwait;
pub mod tgkill;

pub use kill::syscall_kill;
pub use sigaction::syscall_rt_sigaction;
pub use sigpending::syscall_rt_sigpending;
pub use sigprocmask::syscall_rt_sigprocmask;
pub use sigqueueinfo::{syscall_rt_sigqueueinfo, syscall_rt_tgsigqueueinfo};
pub use sigreturn::syscall_rt_sigreturn;
pub use sigsuspend::syscall_rt_sigsuspend;
pub use sigtimedwait::syscall_rt_sigtimedwait;
pub use tgkill::syscall_tgkill;
