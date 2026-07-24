pub mod arch_prctl;
pub mod clone;
pub mod credentials;
pub mod execve;
pub mod exit;
pub mod fork;
pub mod pid;
pub mod wait4;
pub mod waitid;

pub use arch_prctl::syscall_arch_prctl;
pub use clone::syscall_clone;
pub use credentials::{
    syscall_getegid, syscall_geteuid, syscall_getgid, syscall_getresgid, syscall_getresuid,
    syscall_getuid, syscall_setfsgid, syscall_setfsuid, syscall_setgid, syscall_setregid,
    syscall_setresgid, syscall_setresuid, syscall_setreuid, syscall_setuid,
};
pub use execve::syscall_execve;
pub use exit::syscall_exit;
pub use fork::syscall_fork;
pub use pid::{
    syscall_getpgid, syscall_getpid, syscall_getppid, syscall_getsid, syscall_setpgid,
    syscall_setsid,
};
pub use wait4::syscall_wait4;
pub use waitid::syscall_waitid;