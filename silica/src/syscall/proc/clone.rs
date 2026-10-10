// SPDX-License-Identifier: GPL-2.0

//! The `clone` and `set_tid_address` system calls (Syscall #56 and #218 on x86_64).

use alloc::sync::{Arc, Weak};
use ostd::{
    arch::cpu::context::UserContext,
    sync::SpinLock,
    task::TaskOptions,
    user::UserContextApi,
};

use crate::{
    api::{
        errno::{Errno, Result},
        signal::{SigActs, SigSet, SigStack},
    },
    proc::{
        Proc,
        thread::{Thread, ThreadFlags, ThreadInner, ThreadState, alloc_tid},
        tree::{PID_ALLOCATOR, allproc_insert},
    },
    utils::userspace::user_ptr::UserPtr,
    vm::vmar::Vmar,
};

pub const CSIGNAL: usize = 0x0000_00ff;
pub const CLONE_VM: usize = 0x0000_0100;
pub const CLONE_FS: usize = 0x0000_0200;
pub const CLONE_FILES: usize = 0x0000_0400;
pub const CLONE_SIGHAND: usize = 0x0000_0800;
pub const CLONE_THREAD: usize = 0x0001_0000;
pub const CLONE_SETTLS: usize = 0x0008_0000;
pub const CLONE_PARENT_SETTID: usize = 0x0010_0000;
pub const CLONE_CHILD_CLEARTID: usize = 0x0020_0000;
pub const CLONE_CHILD_SETTID: usize = 0x0100_0000;

fn write_user_i32(vmar: &Vmar, ptr: usize, val: i32) -> Result<()> {
    if ptr == 0 {
        return Ok(());
    }
    UserPtr::new(ptr).write_bytes(vmar, &val.to_ne_bytes())
}

/// System call `clone(flags, stack, parent_tid, child_tid, tls)` (Syscall #56).
pub fn sys_clone(
    flags: usize,
    stack: usize,
    parent_tid_ptr: usize,
    child_tid_ptr: usize,
    tls: usize,
    ctx: &mut UserContext,
) -> Result<usize> {
    let caller_td = Thread::current().ok_or(Errno::ESRCH)?;
    let caller_proc = caller_td.proc().ok_or(Errno::ESRCH)?;
    let caller_vmar = caller_proc.vmspace();

    let is_thread = (flags & CLONE_THREAD) != 0;

    let mut child_ctx = ctx.clone();
    // In child execution, clone returns 0.
    child_ctx.general_regs_mut().rax = 0;
    if stack != 0 {
        child_ctx.set_stack_pointer(stack);
    }

    let child_tid = alloc_tid();
    let tid_val = child_tid.0.get() as i32;

    let fs_base = if (flags & CLONE_SETTLS) != 0 {
        tls
    } else {
        caller_td.fs_base()
    };

    let clear_child_tid = if (flags & CLONE_CHILD_CLEARTID) != 0 {
        child_tid_ptr
    } else {
        0
    };

    if is_thread {
        // --- Thread creation inside the same process ---
        let child_proc_weak = Arc::downgrade(&caller_proc);
        let child_vmar = Arc::clone(&caller_vmar);

        let child_thread = Arc::new_cyclic(|weak_thread: &Weak<Thread>| {
            let weak_td_clone = weak_thread.clone();
            let child_ctx_clone = child_ctx.clone();

            let options = TaskOptions::new(move || {
                if let Some(td) = weak_td_clone.upgrade() {
                    let hooks = crate::proc::thread::ThreadHooks::new(&td);
                    td.user_loop(child_ctx_clone, hooks);
                }
            })
            .data(weak_thread.clone())
            .local_data(child_vmar);

            let task = crate::proc::thread::build_task(options, 0);

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

        if (flags & CLONE_PARENT_SETTID) != 0 && parent_tid_ptr != 0 {
            write_user_i32(&caller_vmar, parent_tid_ptr, tid_val)?;
        }
        if (flags & CLONE_CHILD_SETTID) != 0 && child_tid_ptr != 0 {
            write_user_i32(&caller_vmar, child_tid_ptr, tid_val)?;
        }

        child_thread.run();
        Ok(tid_val as usize)
    } else {
        // --- Process creation (fork / vfork) ---
        let child_pid = PID_ALLOCATOR.lock().allocate()?;
        let pid_val = child_pid.as_u32() as i32;

        let child_vmar = if (flags & CLONE_VM) != 0 {
            Arc::clone(&caller_vmar)
        } else {
            Vmar::fork_from(&caller_vmar)
        };

        let child_fd_table = if (flags & CLONE_FILES) != 0 {
            Arc::clone(&caller_proc.fd_table)
        } else {
            Arc::new(SpinLock::new(caller_proc.fd_table.lock().clone_table()))
        };

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

        let child_proc_weak = Arc::downgrade(&child_proc);
        let child_vmar_clone = Arc::clone(&child_vmar);

        let child_thread = Arc::new_cyclic(|weak_thread: &Weak<Thread>| {
            let weak_td_clone = weak_thread.clone();
            let child_ctx_clone = child_ctx.clone();

            let options = TaskOptions::new(move || {
                if let Some(td) = weak_td_clone.upgrade() {
                    let hooks = crate::proc::thread::ThreadHooks::new(&td);
                    td.user_loop(child_ctx_clone, hooks);
                }
            })
            .data(weak_thread.clone())
            .local_data(child_vmar_clone);

            let task = crate::proc::thread::build_task(options, 0);

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

        allproc_insert(Arc::clone(&child_proc));
        caller_proc.inner.lock().children.push(child_pid);

        if (flags & CLONE_PARENT_SETTID) != 0 && parent_tid_ptr != 0 {
            write_user_i32(&caller_vmar, parent_tid_ptr, pid_val)?;
        }
        if (flags & CLONE_CHILD_SETTID) != 0 && child_tid_ptr != 0 {
            write_user_i32(&child_vmar, child_tid_ptr, pid_val)?;
        }

        child_thread.run();
        Ok(pid_val as usize)
    }
}

/// System call `set_tid_address(int *tidptr)` (Syscall #218).
pub fn sys_set_tid_address(tidptr: usize, _ctx: &mut UserContext) -> Result<usize> {
    let td = Thread::current().ok_or(Errno::ESRCH)?;
    td.inner.lock().clear_child_tid = tidptr;
    Ok(td.tid.0.get() as usize)
}
