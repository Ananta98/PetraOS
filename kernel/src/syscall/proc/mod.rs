pub mod arch_prctl;
pub mod clone;
pub mod credentials;
pub mod execve;
pub mod exit;
pub mod fork;
pub mod futex;
pub mod getrandom;
pub mod pid;
pub mod ptrace;
pub mod sys_info;
pub mod tid;
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
pub use futex::syscall_futex;
pub use getrandom::syscall_getrandom;
pub use pid::{
    syscall_getpgid, syscall_getpid, syscall_getppid, syscall_getsid, syscall_setpgid,
    syscall_setsid,
};
pub use ptrace::syscall_ptrace;
pub use sys_info::{syscall_getrlimit, syscall_prlimit64, syscall_sysinfo, syscall_uname};
pub use tid::{
    syscall_exit_group, syscall_gettid, syscall_set_robust_list, syscall_set_tid_address,
};
pub use wait4::syscall_wait4;
pub use waitid::syscall_waitid;
