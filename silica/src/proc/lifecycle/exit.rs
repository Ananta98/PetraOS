// SPDX-License-Identifier: GPL-2.0

//! Process termination and reaping.
//!
//! Implements `exit1` for process termination, zombie state transitions,
//! orphan reparenting to designated reapers, and resource reclamation via `proc_reap`.

use alloc::{sync::Arc, vec::Vec};

use crate::{
    api::{
        errno::{Errno, Result},
        signal::Signal,
    },
    proc::{
        ExitStatus, Proc, ProcState,
        thread::{Thread, ThreadState},
        types::{
            pid::Pid,
            table::{
                PID_ALLOCATOR, allproc_find, allproc_remove, tree_find_reaper,
                tree_reparent_children,
            },
        },
    },
};

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
    crate::drivers::char::tty::on_proc_exit(&proc);

    // 3. Reparent orphan children to reaper
    if !children_to_reparent.is_empty() {
        tree_reparent_children(&children_to_reparent, reaper_pid);
        for &child_pid in &children_to_reparent {
            if let Some(child) = allproc_find(child_pid) {
                if child.state() == ProcState::Zombie {
                    if let Some(reaper) = allproc_find(reaper_pid) {
                        reaper.post_signal(Signal::SIGCHLD);
                        reaper.child_waitq.wake_all();
                        super::wait::notify_child_events();
                    }
                } else {
                    crate::proc::job::notify_orphaned_pgrp_if_stopped(child.pgid());
                }
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
        thread.handle_clear_child_tid(&proc);
        let mut td_inner = thread.inner.lock();
        td_inner.state = ThreadState::Dead;
        td_inner.flags.insert(
            crate::proc::thread::ThreadFlags::TDF_ASTPENDING
                | crate::proc::thread::ThreadFlags::KILL_PENDING,
        );
    }
    // Wake any threads parked on stop_queue and any parent sleeping in vfork
    proc.stop_queue.wake_all();
    proc.wake_vfork();
    if let Some(parent_pid) = ppid && let Some(parent) = allproc_find(parent_pid) {
        parent.wake_vfork();
    }

    // Report the status so a process's end is visible in the log. Nothing else
    // does: the zombie keeps `xstat` until a parent reaps it, and a process with
    // no parent is never reaped at all.
    ostd::info!("proc: pid {} exited ({})", proc.pid, status);
}

/// Terminates a single thread within a process.
///
/// Follows Linux `exit(2)` semantics:
/// If other threads remain alive in the process, only the calling thread is terminated,
/// its `clear_child_tid` pointer cleared in userspace, and waiters on the futex are woken.
/// If this is the last thread in the process, the entire process terminates via `exit1`.
pub fn exit_thread(td: &Thread, code: i32) {
    let proc = td.proc().expect("thread must belong to process");

    let is_last_thread = {
        let mut inner = proc.inner.lock();
        if inner.state == ProcState::Zombie {
            drop(inner);
            td.handle_clear_child_tid(&proc);
            let mut td_inner = td.inner.lock();
            td_inner.state = ThreadState::Dead;
            return;
        }

        let other_threads_alive = inner
            .threads
            .iter()
            .any(|t| t.tid != td.tid && t.inner.lock().state != ThreadState::Dead);

        if other_threads_alive {
            // Sibling threads are still running: remove this thread from the process's thread list
            inner.threads.retain(|t| t.tid != td.tid);
            false
        } else {
            // No living siblings; this is the last surviving thread of the process
            true
        }
    };

    if is_last_thread {
        exit1(td, ExitStatus::Exited(code));
    } else {
        // Individual thread cleanup
        td.handle_clear_child_tid(&proc);
        let mut td_inner = td.inner.lock();
        td_inner.state = ThreadState::Dead;
        td_inner.flags.insert(
            crate::proc::thread::ThreadFlags::TDF_ASTPENDING
                | crate::proc::thread::ThreadFlags::KILL_PENDING,
        );
        ostd::info!("thread: tid {} exited ({})", td.tid, code);
    }
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

    // Accumulate child's rusage and remove from parent's children list
    {
        let child_inner = child.inner.lock();
        let mut p_inner = parent.inner.lock();
        p_inner.children_rusage.accumulate(&child_inner.children_rusage);
        let child_rss_kb = (child.vmspace().total_mapped_size() / 1024) as i64;
        if child_rss_kb > p_inner.children_rusage.ru_maxrss {
            p_inner.children_rusage.ru_maxrss = child_rss_kb;
        }
        p_inner.children.retain(|&p| p != child_pid);
    }

    // Remove from global process tree and free PID
    allproc_remove(child_pid);
    PID_ALLOCATOR.lock().free(child_pid);

    Ok((child_pid, status))
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::{prelude::ktest, sync::SpinLock, task::TaskOptions};
    use crate::{
        api::{
            cred::Ucred,
            limit::Plimit,
            signal::{SigActs, SigSet, SigStack},
        },
        fs::fd::Filedesc,
        proc::{
            thread::{ThreadFlags, ThreadInner, alloc_tid},
            types::table::allproc_insert,
        },
        vm::{flags::MmapFlags, perms::VmPerms, vmar::Vmar},
    };

    fn create_test_proc() -> Arc<Proc> {
        let pid = PID_ALLOCATOR.lock().allocate().expect("allocate pid");
        let vmar = Vmar::new();
        let fd_table = Arc::new(SpinLock::new(Filedesc::new()));
        let cred = Arc::new(Ucred::root());
        let limit = Arc::new(Plimit::default_limits());
        let sigacts = Arc::new(SpinLock::new(SigActs::new()));
        let proc = Proc::new(
            pid,
            None,
            vmar,
            fd_table,
            cred,
            limit,
            sigacts,
            "test_proc",
        );
        allproc_insert(Arc::clone(&proc));
        proc
    }

    fn create_test_thread(proc: &Arc<Proc>, clear_tid: usize) -> Arc<Thread> {
        let tid = alloc_tid();
        let proc_weak = Arc::downgrade(proc);
        let name_buf = crate::proc::comm_from_name("test_thread");
        let task = Arc::new(TaskOptions::new(|| {}).build().expect("build task"));

        let thread = Arc::new(Thread {
            tid,
            td_proc: proc_weak,
            td_kstack: task,
            inner: SpinLock::new(ThreadInner {
                state: ThreadState::CanRun,
                flags: ThreadFlags::TDF_KTHREAD,
                sigmask: SigSet::initial(),
                sigqueue: crate::proc::signal::SigQueue::new(),
                wchan: None,
                wmesg: "",
                name: name_buf,
                user_ctx: None,
                altstack: SigStack::default(),
                handler_mask: SigSet::empty(),
                fs_base: 0,
                stopped_by: None,
                handler_frame: None,
                clear_child_tid: clear_tid,
            }),
        });

        proc.add_thread(Arc::clone(&thread));
        thread
    }

    #[ktest]
    fn thread_exit_clear_child_tid_and_futex_wake() {
        let proc = create_test_proc();
        let vmar = proc.vmspace();
        let test_addr = 0x0000_1000_0000usize;

        let mapped = vmar
            .mmap_anonymous(
                test_addr,
                4096,
                VmPerms::READ | VmPerms::WRITE,
                MmapFlags::ANONYMOUS | MmapFlags::PRIVATE,
            )
            .expect("mmap anonymous");

        vmar.populate_range(&(mapped..mapped + ostd::mm::PAGE_SIZE))
            .expect("populate range");
        vmar.activate();

        let tid_val = 12345i32;
        crate::utils::userspace::user_ptr::UserPtr::new(mapped)
            .write_bytes(&vmar, &tid_val.to_ne_bytes())
            .expect("write tid");

        let thread = create_test_thread(&proc, mapped);
        assert_eq!(thread.inner.lock().clear_child_tid, mapped);

        thread.handle_clear_child_tid(&proc);

        assert_eq!(thread.inner.lock().clear_child_tid, 0);

        let mut read_bytes = [0u8; 4];
        crate::utils::userspace::user_ptr::UserPtr::new(mapped)
            .read_bytes(&vmar, &mut read_bytes)
            .expect("read bytes");
        assert_eq!(i32::from_ne_bytes(read_bytes), 0);

        thread.handle_clear_child_tid(&proc);
        assert_eq!(thread.inner.lock().clear_child_tid, 0);
    }

    #[ktest]
    fn thread_exit_multithread_vs_last_thread() {
        let proc = create_test_proc();
        let thread1 = create_test_thread(&proc, 0);
        let thread2 = create_test_thread(&proc, 0);

        assert_eq!(proc.inner.lock().threads.len(), 2);
        assert_eq!(proc.state(), ProcState::Alive);

        exit_thread(&thread1, 0);

        assert_eq!(thread1.inner.lock().state, ThreadState::Dead);
        assert_eq!(proc.inner.lock().threads.len(), 1);
        assert_eq!(proc.inner.lock().threads[0].tid, thread2.tid);
        assert_eq!(proc.state(), ProcState::Alive);

        exit_thread(&thread2, 42);

        assert_eq!(thread2.inner.lock().state, ThreadState::Dead);
        assert_eq!(proc.state(), ProcState::Zombie);
        assert_eq!(proc.inner.lock().xstat, Some(ExitStatus::Exited(42)));
    }

    #[ktest]
    fn thread_exit_group_terminates_all_threads() {
        let proc = create_test_proc();
        let thread1 = create_test_thread(&proc, 0);
        let thread2 = create_test_thread(&proc, 0);

        assert_eq!(proc.inner.lock().threads.len(), 2);
        assert_eq!(proc.state(), ProcState::Alive);

        exit1(&thread1, ExitStatus::Exited(99));

        assert_eq!(thread1.inner.lock().state, ThreadState::Dead);
        assert_eq!(thread2.inner.lock().state, ThreadState::Dead);
        assert_eq!(proc.state(), ProcState::Zombie);
        assert_eq!(proc.inner.lock().xstat, Some(ExitStatus::Exited(99)));
    }
}
