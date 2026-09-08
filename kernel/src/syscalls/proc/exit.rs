//! Process termination system calls (`exit`, `exit_group`).

use crate::arch::syscall::syscall::SyscallFrame;
use crate::proc::{Process, ProcessId, ProcessState, ThreadState};
use crate::syscalls::SyscallResult;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use crate::sync::Mutex;

/// Reparent orphaned children of `dead_pid` to `init` (PID 1) to prevent leaked orphans.
///
/// POSIX requires children of a terminating process to be adopted by `init`.
/// If `init` does not exist or the dying process is `init` itself, children
/// are orphaned to PID 0.
///
/// Lock ordering: `init` (PID 1) → child (higher PID) matches `wait4` path
/// (`init` lock → child lock) and avoids deadlock.
fn reparent_orphans(
    dead_pid: ProcessId,
    children: BTreeMap<ProcessId, Arc<Mutex<Process>>>,
) {
    if children.is_empty() {
        return;
    }

    // init exiting must not reparent to itself.
    if dead_pid == ProcessId::new(1) {
        for (_, child) in children {
            child.lock().ppid = ProcessId::new(0);
        }
        return;
    }

    let init_pid = ProcessId::new(1);
    if let Some(init_arc) = crate::proc::find_process(init_pid) {
        // Acquire init first to preserve lock ordering init → child.
        let mut init = init_arc.lock();
        let mut has_zombie = false;

        // First pass: check zombie state while holding init → child ordering.
        // We do check and ppid update in the same init-held section to keep
        // ordering consistent.
        for (_, child_arc) in &children {
            // SAFETY: init lock held, now taking child lock => init → child.
            let mut child = child_arc.lock();
            if child.state == ProcessState::Zombie {
                has_zombie = true;
            }
            child.ppid = init_pid;
            // child lock drops here before next iteration, init remains held.
        }

        // Adopt into init's children map.
        for (pid, child_arc) in children {
            init.children.insert(pid, child_arc);
        }

        // If any reparented child is already zombie, notify new parent so
        // a blocking `wait4` on init can observe it.
        if has_zombie {
            let _ = init.send_signal(crate::ipc::signal::SIGCHLD);
        }
    } else {
        // No init found: orphan to PID 0.
        for (_, child_arc) in children {
            child_arc.lock().ppid = ProcessId::new(0);
        }
    }
}

/// Notify the parent of `ppid` with `SIGCHLD` if it still exists.
#[inline]
fn send_sigchld_to_parent(ppid: ProcessId) {
    if ppid.as_u64() == 0 {
        return;
    }
    if let Some(parent_arc) = crate::proc::find_process(ppid) {
        let mut parent = parent_arc.lock();
        let _ = parent.send_signal(crate::ipc::signal::SIGCHLD);
    }
}

/// Full process (thread-group) termination.
///
/// Marks the entire process and all its threads as `Zombie`, detaches shared
/// resources, reparents orphans to `init`, sends `SIGCHLD` to the parent, and
/// never returns. The current CPU switches away via `schedule(false)`; if no
/// runnable thread remains it enters `idle()` which never returns. This prevents
/// `iretq` in `Syscall.S` from resuming a dead user context.
pub(crate) fn do_exit_group(code: i32) -> ! {
    // Single capture of current thread to avoid TOCTOU on cpu_id() / migration.
    let current_thread = crate::proc::current_thread();

    // Derive owning process from the thread's weak reference first, fallback
    // to current_process() lookup. This keeps thread/process pairing consistent.
    let proc_arc = current_thread
        .as_ref()
        .and_then(|t| t.lock().process.upgrade())
        .or_else(crate::proc::current_process);

    let (ppid, pid, children) = if let Some(proc_arc) = proc_arc {
        let (ppid, pid, children) = {
            let mut proc = proc_arc.lock();
            let ppid = proc.ppid;
            let pid = proc.pid;
            // `Process::exit` handles: state=Zombie, exit_code, SHM detach,
            // and marking all non-current threads Zombie + dequeue.
            proc.exit(code);
            // Extract children for reparenting after dropping proc lock
            // to avoid holding proc → init ordering across the call.
            let children = core::mem::take(&mut proc.children);
            (ppid, pid, children)
        };
        (ppid, pid, children)
    } else {
        (ProcessId::new(0), ProcessId::new(0), BTreeMap::new())
    };

    // Ensure the current thread is Zombie with correct exit_code.
    // `Process::exit` already set state for all threads but not exit_code.
    if let Some(thread_arc) = current_thread {
        let mut t = thread_arc.lock();
        if t.state != ThreadState::Zombie {
            t.state = ThreadState::Zombie;
        }
        // Preserve first exit code if already set (signal vs explicit exit)
        if t.exit_code.is_none() {
            t.exit_code = Some((code & 0xFF) as u32);
        }
    }

    // POSIX orphan reparenting: must happen before SIGCHLD so new parent
    // can immediately wait on already-zombie children.
    reparent_orphans(pid, children);

    // Notify original parent.
    send_sigchld_to_parent(ppid);

    // SAFETY: `schedule(false)` clears `current` slot and picks next runnable.
    // If a next thread exists, `arch_switch_context` never returns to this
    // stack. If no thread is runnable, `schedule` enters `idle()` (hlt loop)
    // which is `!`; otherwise we fall through to the idle fallback below.
    crate::sched::schedule(false);

    // Fallback if scheduler returned (None,None edge or same-thread check).
    // Keep CPU in low-power halt until next interrupt / never return.
    loop {
        crate::arch::idle();
    }
}

/// Thread-only termination for `sys_exit`.
///
/// POSIX `exit(2)` terminates only the calling thread when the thread group
/// contains multiple threads. Only when this is the last thread does the
/// process become Zombie (delegating to `do_exit_group`).
fn do_exit_thread(code: i32) -> ! {
    let current_thread = crate::proc::current_thread();

    // No current thread (e.g. kernel context) -> just deschedule.
    let Some(thread_arc) = current_thread else {
        crate::sched::schedule(false);
        loop {
            crate::arch::idle();
        }
    };

    // Snapshot TID without holding thread lock across proc lock.
    let tid = { thread_arc.lock().tid };

    // Determine owning process consistently from thread weak.
    let proc_arc = {
        let t = thread_arc.lock();
        t.process.upgrade()
    }
    .or_else(crate::proc::current_process);

    let Some(proc_arc) = proc_arc else {
        // No process container: thread-only zombie then deschedule.
        {
            let mut t = thread_arc.lock();
            t.state = ThreadState::Zombie;
            t.exit_code = Some((code & 0xFF) as u32);
        }
        crate::sched::schedule(false);
        loop {
            crate::arch::idle();
        }
    };

    // Check if this is the last thread atomically with removal to avoid
    // race where two threads both see len==2 and both do thread-only exit
    // leaving 0 threads and no Zombie.
    let is_last = {
        let mut proc = proc_arc.lock();
        if proc.threads.len() <= 1 {
            true
        } else {
            proc.threads.remove(&tid);
            false
        }
    };

    if is_last {
        // Last thread exiting => whole process terminates with group semantics.
        do_exit_group(code)
    }

    {
        let mut t = thread_arc.lock();
        t.state = ThreadState::Zombie;
        t.exit_code = Some((code & 0xFF) as u32);
    }

    // Ensure not queued as runnable (current is in `current` slot, but dequeue
    // covers the case of spurious queue membership after races).
    crate::sched::remove_thread(tid);

    // No SIGCHLD and no reparent for thread-only exit.

    crate::sched::schedule(false);
    loop {
        crate::arch::idle();
    }
}

/// Compatibility alias kept for internal callers: full process exit.
pub(crate) fn do_exit(code: i32) -> ! {
    do_exit_group(code)
}

/// `sys_exit` (SYS_EXIT = 60)
/// Terminate the calling thread. If it is the last thread in the thread
/// group, the whole process becomes Zombie (POSIX `exit` semantics).
pub fn sys_exit(frame: &mut SyscallFrame) -> SyscallResult {
    let code = frame.arg1() as i32;
    log::debug!("sys_exit called with status code {}", code);
    do_exit_thread(code)
}

/// `sys_exit_group` (SYS_EXIT_GROUP = 231)
/// Exit all threads in a process (POSIX `exit_group`).
pub fn sys_exit_group(frame: &mut SyscallFrame) -> SyscallResult {
    let code = frame.arg1() as i32;
    log::debug!("sys_exit_group called with status code {}", code);
    do_exit_group(code)
}
