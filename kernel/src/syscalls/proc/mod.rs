//! Process management system call handlers and common abstractions.

pub mod cred;
pub mod exec;
pub mod exit;
pub mod fork;
pub mod prctl;
pub mod rlimit;
pub mod tid;
pub mod wait;
pub mod r#yield;

// ── Re-exports of system call entry points ──────────────────────────────────
pub use cred::{
    sys_getegid, sys_geteuid, sys_getgid, sys_getgroups, sys_getpgid, sys_getpgrp, sys_getpid,
    sys_getppid, sys_getresgid, sys_getresuid, sys_getsid, sys_getuid, sys_setfsgid, sys_setfsuid,
    sys_setgid, sys_setgroups, sys_setpgid, sys_setregid, sys_setresgid, sys_setresuid,
    sys_setreuid, sys_setsid, sys_setuid,
};
pub use exec::sys_execve;
pub use exit::{sys_exit, sys_exit_group};
pub use fork::{sys_fork, sys_vfork};
pub use prctl::sys_prctl;
pub use r#yield::sys_yield;
pub use rlimit::{
    sys_getrlimit, sys_getrusage, sys_prlimit64, sys_setrlimit, LinuxRusage, RLimit64, RUsage,
};
pub use tid::{sys_gettid, sys_set_tid_address};
pub use wait::sys_wait4;
