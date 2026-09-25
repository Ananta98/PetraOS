//! Process termination system calls (`exit`, `exit_group`).

use crate::syscalls::{SyscallResult, wrap_syscall};

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

/// `sys_exit` (SYS_EXIT = 60)
/// Terminate the calling thread or process.
#[wrap_syscall]
pub fn sys_exit(code: i32) -> SyscallResult {
    log::debug!("sys_exit called with status code {}", code);
    do_exit(code)
}

/// `sys_exit_group` (SYS_EXIT_GROUP = 231)
/// Exit all threads in a process.
#[wrap_syscall]
pub fn sys_exit_group(code: i32) -> SyscallResult {
    log::debug!("sys_exit_group called with status code {}", code);
    do_exit(code)
}
