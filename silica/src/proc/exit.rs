// SPDX-License-Identifier: GPL-2.0

//! Process termination and reaping.
//!
//! Implements `exit1` for process termination, zombie state transitions,
//! orphan reparenting to designated reapers, and resource reclamation via `proc_reap`.

use super::{
    ExitStatus, Proc, ProcState,
    thread::{Thread, ThreadState},
    tree::{
        PID_ALLOCATOR, Pid, allproc_find, allproc_remove, tree_find_reaper, tree_reparent_children,
    },
};
use crate::api::{
    errno::{Errno, Result},
    signal::Signal,
};
use crate::utils::userspace::user_ptr::UserPtr;
use alloc::{sync::Arc, vec::Vec};

/// Terminates the current process.
pub fn exit1(td: &Thread, status: ExitStatus) {
    let proc = td.proc().expect("thread must belong to process");

    // 1. Transition process state to Zombie under lock.
    //
    // The reaper is only *read* here. Resolving it needs the ancestors' locks, so
    // it must happen after this guard is dropped; doing it inside the block would
    // re-lock `proc.inner` and deadlock, since `SpinLock` is not reentrant.
    let (children_to_reparent, ppid, reaper) = {
        let mut inner = proc.inner.lock();
        if inner.state == ProcState::Zombie {
            return;
        }
        inner.state = ProcState::Zombie;
        inner.xstat = Some(status);

        let children = core::mem::take(&mut inner.children);
        (children, inner.ppid, inner.reaper)
    };
    let reaper_pid = reaper.unwrap_or_else(|| tree_find_reaper(ppid));

    // 2. Tear down open files and detach SysV IPC resources (shm, sem, msg)
    proc.fd_table.lock().close_all();
    crate::ipc::shm::on_proc_exit(&proc);
    crate::ipc::sem::on_proc_exit(&proc);
    crate::ipc::msg::on_proc_exit(&proc);
    crate::sched::on_proc_exit(proc.pid);

    // 3. Reparent orphan children to reaper
    if !children_to_reparent.is_empty() {
        tree_reparent_children(&children_to_reparent, reaper_pid);
        for &child_pid in &children_to_reparent {
            let is_zombie = allproc_find(child_pid).is_some_and(|c| c.state() == ProcState::Zombie);
            if is_zombie && let Some(reaper) = allproc_find(reaper_pid) {
                reaper.post_signal(Signal::SIGCHLD);
                super::wait::notify_child_events();
            }
        }
    }

    // 4. Notify parent process via SIGCHLD and wake wait queues
    if let Some(parent) = ppid.and_then(allproc_find) {
        parent.post_signal(Signal::SIGCHLD);
        parent.child_waitq.wake_all();
        super::wait::notify_child_events();
    }

    // 5. Mark every thread of the process dead, not just the caller, and clean up.
    //
    // `proc_reap` drops the last strong `Arc<Proc>` once the zombie is collected,
    // and a surviving sibling's `Thread::td_proc` is a `Weak`. Sibling threads are
    // flagged with AST and KILL_PENDING so they exit on return to kernel mode.
    let threads: Vec<_> = proc.inner.lock().threads.clone();
    for thread in &threads {
        let clear_tid = thread.inner.lock().clear_child_tid;
        if clear_tid != 0 {
            let zero = 0i32.to_ne_bytes();
            let _ = UserPtr::new(clear_tid).write_bytes(&proc.vmspace(), &zero);
        }
        let mut td_inner = thread.inner.lock();
        td_inner.state = ThreadState::Dead;
        td_inner.flags.insert(
            crate::proc::thread::ThreadFlags::TDF_ASTPENDING
                | crate::proc::thread::ThreadFlags::KILL_PENDING,
        );
    }
    // Wake any threads parked on stop_queue so they terminate immediately
    proc.stop_queue.wake_all();

    // Report the status so a process's end is visible in the log. Nothing else
    // does: the zombie keeps `xstat` until a parent reaps it, and a process with
    // no parent is never reaped at all.
    ostd::info!("proc: pid {} exited ({})", proc.pid, status);
}

/// Frees zombie metadata and removes process from `allproc`.
pub fn proc_reap(parent: &Arc<Proc>, child_pid: Pid) -> Result<(Pid, ExitStatus)> {
    let child = allproc_find(child_pid).ok_or(Errno::ESRCH)?;

    // Verify parentage
    let status = {
        let child_inner = child.inner.lock();
        if child_inner.ppid != Some(parent.pid) {
            crate::return_errno!(ECHILD, "process is not a child of caller");
        }
        if child_inner.state != ProcState::Zombie {
            crate::return_errno!(EINVAL, "child process is not a zombie");
        }
        child_inner.xstat.unwrap_or(ExitStatus::Exited(0))
    };

    // Clean up remaining threads from the child process
    child.inner.lock().threads.clear();

    // Remove from parent's children list
    {
        let mut p_inner = parent.inner.lock();
        p_inner.children.retain(|&p| p != child_pid);
    }

    // Remove from global process tree and free PID
    allproc_remove(child_pid);
    PID_ALLOCATOR.lock().free(child_pid);

    Ok((child_pid, status))
}
