use super::{SyscallError, SyscallResult, UserCStr, UserPtr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::mm::vmm::paging::PageTable;
use crate::proc::ProcessId;

// ── Modular syscall submodules ──────────────────────────────────────────
pub mod r#yield;
pub mod getpid;
pub mod getppid;
pub mod getpgrp;
pub mod setpgid;
pub mod getuid;
pub mod getgid;
pub mod setuid;
pub mod setgid;
pub mod geteuid;
pub mod getegid;
pub mod setsid;
pub mod getgroups;
pub mod getrlimit;
pub mod setrlimit;
pub mod prlimit64;
pub mod fork;
pub mod vfork;
pub mod execve;
pub mod wait4;
pub mod exit;
pub mod exit_group;

pub use r#yield::sys_yield;
pub use getpid::sys_getpid;
pub use getppid::sys_getppid;
pub use getpgrp::sys_getpgrp;
pub use setpgid::sys_setpgid;
pub use getuid::sys_getuid;
pub use getgid::sys_getgid;
pub use setuid::sys_setuid;
pub use setgid::sys_setgid;
pub use geteuid::sys_geteuid;
pub use getegid::sys_getegid;
pub use setsid::sys_setsid;
pub use getgroups::sys_getgroups;
pub use getrlimit::sys_getrlimit;
pub use setrlimit::sys_setrlimit;
pub use prlimit64::sys_prlimit64;
pub use fork::sys_fork;
pub use vfork::sys_vfork;
pub use execve::sys_execve;
pub use wait4::sys_wait4;
pub use exit::sys_exit;
pub use exit_group::sys_exit_group;


/// Linux 64-bit resource limit structure.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct RLimit64 {
    pub rlim_cur: u64,
    pub rlim_max: u64,
}

pub(crate) const RLIM_INFINITY: u64 = !0u64;

pub(crate) fn get_default_rlimit(resource: i32) -> RLimit64 {
    match resource {
        3 /* RLIMIT_STACK */ => RLimit64 {
            rlim_cur: 8 * 1024 * 1024,
            rlim_max: 64 * 1024 * 1024,
        },
        7 /* RLIMIT_NOFILE */ => RLimit64 {
            rlim_cur: 1024,
            rlim_max: 4096,
        },
        6 /* RLIMIT_NPROC */ => RLimit64 {
            rlim_cur: 4096,
            rlim_max: 4096,
        },
        _ => RLimit64 {
            rlim_cur: RLIM_INFINITY,
            rlim_max: RLIM_INFINITY,
        },
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct RUsage {
    pub ru_utime: crate::syscalls::time::TimeVal,
    pub ru_stime: crate::syscalls::time::TimeVal,
    pub ru_maxrss: i64,
    pub ru_ixrss: i64,
    pub ru_idrss: i64,
    pub ru_isrss: i64,
    pub ru_minflt: i64,
    pub ru_majflt: i64,
    pub ru_nswap: i64,
    pub ru_inblock: i64,
    pub ru_oublock: i64,
    pub ru_msgsnd: i64,
    pub ru_msgrcv: i64,
    pub ru_nsignals: i64,
    pub ru_nvcsw: i64,
    pub ru_nivcsw: i64,
}

/// Common exit path shared by `sys_exit` and `sys_exit_group`.
///
/// This function never returns: it marks the current thread and process as zombie,
/// then yields the CPU via `schedule(false)`. If no other runnable thread exists,
/// it falls into the idle loop. Either path prevents `iretq` from firing into a
/// dead user-space context.
pub(crate) fn do_exit(code: i32) -> ! {
    let ppid_opt = if let Some(proc_arc) = crate::proc::current_process() {
        let mut proc = proc_arc.lock();
        proc.exit(code);
        proc.ppid
    } else {
        crate::proc::ProcessId(0)
    };

    if let Some(thread_arc) = crate::proc::current_thread() {
        let mut t = thread_arc.lock();
        t.state = crate::proc::ThreadState::Zombie;
        t.exit_code = Some(code as u32);
    }

    if ppid_opt.as_u64() > 0 {
        if let Some(parent_arc) = crate::proc::find_process(ppid_opt) {
            let mut parent = parent_arc.lock();
            let _ = parent.send_signal(crate::ipc::signal::SIGCHLD);
        }
    }

    loop {
        crate::sched::schedule(false);
    }
}
