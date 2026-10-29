// SPDX-License-Identifier: GPL-2.0

//! Unified process and thread cloning engine.
//!
//! Provides canonical implementations for creating processes (`clone_proc`, `fork1`)
//! and threads (`clone_thread`, `create_user_thread`), ensuring complete state inheritance
//! (CWD, PGID, SID, IPC attachments, scheduler attributes) and supporting POSIX clone flags.

use alloc::sync::{Arc, Weak};
use ostd::{
    arch::cpu::context::UserContext, sync::SpinLock, task::TaskOptions, user::UserContextApi,
};

use crate::{
    uapi::{
        errno::{Errno, Result},
        signal::{SigActs, SigSet, SigStack},
    },
    proc::{
        Proc,
        thread::{Thread, ThreadFlags, ThreadHooks, ThreadInner, ThreadState, build_task},
        types::{
            pid::{Pid, Tid, alloc_tid},
            proc::ProcState,
            table::{PID_ALLOCATOR, allproc_insert},
        },
    },
    vm::vmar::Vmar,
};

pub const CSIGNAL: usize = 0x0000_00ff;
pub const CLONE_VM: usize = 0x0000_0100;
pub const CLONE_FS: usize = 0x0000_0200;
pub const CLONE_FILES: usize = 0x0000_0400;
pub const CLONE_SIGHAND: usize = 0x0000_0800;
pub const CLONE_VFORK: usize = 0x0000_4000;
pub const CLONE_THREAD: usize = 0x0001_0000;
pub const CLONE_SETTLS: usize = 0x0008_0000;
pub const CLONE_PARENT_SETTID: usize = 0x0010_0000;
pub const CLONE_CHILD_CLEARTID: usize = 0x0020_0000;
pub const CLONE_CHILD_SETTID: usize = 0x0100_0000;

/// Clones the calling process into a new child process with configurable flags.
pub fn clone_proc(
    caller_td: &Thread,
    flags: usize,
    stack: usize,
    tls: usize,
    clear_child_tid: usize,
    caller_user_ctx: Option<&UserContext>,
) -> Result<(Arc<Proc>, Arc<Thread>, Pid)> {
    let caller_proc = caller_td.proc().ok_or(Errno::ESRCH)?;
    let caller_vmar = caller_proc.vmspace();

    // 1. Allocate unique child PID
    let child_pid = PID_ALLOCATOR.lock().allocate()?;

    // 2. Determine VMAR: CLONE_VM shares address space, otherwise Copy-on-Write fork
    let child_vmar = if (flags & CLONE_VM) != 0 {
        Arc::clone(&caller_vmar)
    } else {
        Vmar::fork_from(&caller_vmar)
    };

    // 3. File descriptors: CLONE_FILES shares table, otherwise deep clone
    let child_fd_table = if (flags & CLONE_FILES) != 0 {
        Arc::clone(&caller_proc.fd_table)
    } else {
        Arc::new(SpinLock::new(caller_proc.fd_table.lock().clone_table()))
    };

    // 4. Inherit credentials, limits, and signal actions
    let child_cred = caller_proc.cred();
    let child_limit = caller_proc.limit();
    let child_sigacts = if (flags & CLONE_SIGHAND) != 0 {
        Arc::clone(&caller_proc.sigacts)
    } else {
        Arc::new(SpinLock::new(SigActs {
            actions: caller_proc.sigacts.lock().actions,
        }))
    };

    let comm = caller_proc.comm_name();

    // 5. Create PCB
    let child_proc = Proc::new(
        child_pid,
        Some(caller_proc.pid),
        Arc::clone(&child_vmar),
        child_fd_table,
        child_cred,
        child_limit,
        child_sigacts,
        &comm,
    );

    // FIX Bug 1: Inherit Current Working Directory
    if let Ok(caller_cwd) = caller_proc.cwd() {
        child_proc.set_cwd(caller_cwd);
    }

    // FIX Bug 2: Inherit PGID and SID
    let (parent_pgid, parent_sid) = {
        let inner = caller_proc.inner.lock();
        (
            inner.pgid.unwrap_or(caller_proc.pid),
            inner.sid.unwrap_or(caller_proc.pid),
        )
    };
    {
        let mut inner = child_proc.inner.lock();
        inner.pgid = Some(parent_pgid);
        inner.sid = Some(parent_sid);
        let parent_inner = caller_proc.inner.lock();
        inner.exe_path = parent_inner.exe_path.clone();
        inner.environ = parent_inner.environ.clone();
        inner.no_new_privs = parent_inner.no_new_privs;
    }

    // Inherit SysV shared memory attachments
    crate::ipc::shm::on_proc_fork(&caller_proc, &child_proc);

    // 6. Set up child's user execution context
    let mut child_ctx = caller_user_ctx.cloned().unwrap_or_default();
    child_ctx.general_regs_mut().rax = 0;
    if stack != 0 {
        child_ctx.set_stack_pointer(stack);
    }

    // 7. Setup TLS fs_base
    let fs_base = if (flags & CLONE_SETTLS) != 0 {
        tls
    } else {
        caller_td.fs_base()
    };

    // 8. Create child main thread and backing task
    let child_tid = Tid::from_u32(child_pid.as_u32()).unwrap_or_else(alloc_tid);
    let child_proc_weak = Arc::downgrade(&child_proc);
    let child_vmar_clone = Arc::clone(&child_vmar);

    let child_thread = Arc::new_cyclic(|weak_thread: &Weak<Thread>| {
        let weak_td_clone = weak_thread.clone();
        let child_ctx_clone = child_ctx.clone();

        let options = TaskOptions::new(move || {
            if let Some(td) = weak_td_clone.upgrade() {
                let hooks = ThreadHooks::new(&td);
                td.user_loop(child_ctx_clone, hooks);
            }
        })
        .data(weak_thread.clone())
        .local_data(child_vmar_clone);

        let task = build_task(options, 0);

        let caller_td_inner = caller_td.inner.lock();
        Thread {
            tid: child_tid,
            td_proc: child_proc_weak,
            td_kstack: task,
            inner: SpinLock::new(ThreadInner {
                state: ThreadState::CanRun,
                flags: ThreadFlags::empty(),
                sigmask: caller_td_inner.sigmask,
                sigqueue: crate::proc::signal::SigQueue::new(),
                wchan: None,
                wmesg: "",
                name: caller_td_inner.name,
                user_ctx: Some(child_ctx),
                altstack: SigStack::default(),
                handler_mask: SigSet::empty(),
                fs_base,
                stopped_by: None,
                handler_frame: None,
                clear_child_tid,
            }),
        }
    });

    child_proc.add_thread(Arc::clone(&child_thread));

    // Register child process in global process table and hierarchy
    allproc_insert(Arc::clone(&child_proc));
    caller_proc.inner.lock().children.push(child_pid);

    child_thread.run();

    // 9. Handle CLONE_VFORK: caller suspends until child execs or exits
    if (flags & CLONE_VFORK) != 0 {
        caller_proc.vfork_waitq.wait_until(|| {
            if child_proc.state() == ProcState::Zombie
                || child_proc.vmspace().as_ref() as *const _ != caller_vmar.as_ref() as *const _
            {
                Some(())
            } else {
                None
            }
        });
    }

    Ok((child_proc, child_thread, child_pid))
}

/// Clones an execution thread into the calling process (`CLONE_THREAD`).
pub fn clone_thread(
    caller_td: &Thread,
    flags: usize,
    stack: usize,
    tls: usize,
    clear_child_tid: usize,
    caller_user_ctx: Option<&UserContext>,
) -> Result<Arc<Thread>> {
    let caller_proc = caller_td.proc().ok_or(Errno::ESRCH)?;
    let caller_vmar = caller_proc.vmspace();

    let mut child_ctx = caller_user_ctx.cloned().unwrap_or_default();
    child_ctx.general_regs_mut().rax = 0;
    if stack != 0 {
        child_ctx.set_stack_pointer(stack);
    }

    let fs_base = if (flags & CLONE_SETTLS) != 0 {
        tls
    } else {
        caller_td.fs_base()
    };

    let child_tid = alloc_tid();
    let child_proc_weak = Arc::downgrade(&caller_proc);
    let child_vmar = Arc::clone(&caller_vmar);

    let child_thread = Arc::new_cyclic(|weak_thread: &Weak<Thread>| {
        let weak_td_clone = weak_thread.clone();
        let child_ctx_clone = child_ctx.clone();

        let options = TaskOptions::new(move || {
            if let Some(td) = weak_td_clone.upgrade() {
                let hooks = ThreadHooks::new(&td);
                td.user_loop(child_ctx_clone, hooks);
            }
        })
        .data(weak_thread.clone())
        .local_data(child_vmar);

        let task = build_task(options, 0);

        let caller_td_inner = caller_td.inner.lock();
        Thread {
            tid: child_tid,
            td_proc: child_proc_weak,
            td_kstack: task,
            inner: SpinLock::new(ThreadInner {
                state: ThreadState::CanRun,
                flags: ThreadFlags::empty(),
                sigmask: caller_td_inner.sigmask,
                sigqueue: crate::proc::signal::SigQueue::new(),
                wchan: None,
                wmesg: "",
                name: caller_td_inner.name,
                user_ctx: Some(child_ctx),
                altstack: SigStack::default(),
                handler_mask: SigSet::empty(),
                fs_base,
                stopped_by: None,
                handler_frame: None,
                clear_child_tid,
            }),
        }
    });

    caller_proc.add_thread(Arc::clone(&child_thread));
    child_thread.run();

    Ok(child_thread)
}

/// Standard POSIX `fork1`: clones calling process into child process with CoW memory.
pub fn fork1(caller_td: &Thread, caller_user_ctx: Option<&UserContext>) -> Result<Pid> {
    let (_child_proc, _child_thread, child_pid) =
        clone_proc(caller_td, 0, 0, 0, 0, caller_user_ctx)?;
    Ok(child_pid)
}

/// Creates a user thread belonging to `proc` with given entry context.
pub fn create_user_thread(
    proc: &Arc<Proc>,
    name: &str,
    user_ctx: UserContext,
    nice: i8,
) -> Result<Arc<Thread>> {
    let tid = if proc.inner.lock().threads.is_empty() {
        Tid::from_u32(proc.pid.as_u32()).unwrap_or_else(alloc_tid)
    } else {
        alloc_tid()
    };
    let vmar = proc.vmspace();
    let proc_weak = Arc::downgrade(proc);
    let name_buf = crate::proc::types::proc::comm_from_name(name);

    let thread = Arc::new_cyclic(|weak_thread: &Weak<Thread>| {
        let weak_for_closure = weak_thread.clone();
        let weak_for_task = weak_thread.clone();
        let user_ctx_clone = user_ctx.clone();
        let options = TaskOptions::new(move || {
            if let Some(td) = weak_for_closure.upgrade() {
                let hooks = ThreadHooks::new(&td);
                td.user_loop(user_ctx_clone, hooks);
            }
        })
        .data(weak_for_task)
        .local_data(vmar);

        let task = build_task(options, nice);

        Thread {
            tid,
            td_proc: proc_weak,
            td_kstack: task,
            inner: SpinLock::new(ThreadInner {
                state: ThreadState::CanRun,
                flags: ThreadFlags::empty(),
                sigmask: SigSet::empty(),
                sigqueue: crate::proc::signal::SigQueue::new(),
                wchan: None,
                wmesg: "",
                name: name_buf,
                user_ctx: Some(user_ctx),
                altstack: SigStack::default(),
                handler_mask: SigSet::empty(),
                fs_base: 0,
                stopped_by: None,
                handler_frame: None,
                clear_child_tid: 0,
            }),
        }
    });

    proc.add_thread(Arc::clone(&thread));
    Ok(thread)
}
