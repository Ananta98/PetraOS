// SPDX-License-Identifier: GPL-2.0

//! The `clone` and `set_tid_address` system calls (Syscall #56 and #218 on x86_64).

use ostd::arch::cpu::context::UserContext;

use crate::{
    api::errno::{Errno, Result},
    proc::{
        lifecycle::{
            CLONE_CHILD_CLEARTID, CLONE_CHILD_SETTID, CLONE_PARENT_SETTID, CLONE_THREAD,
            clone_proc, clone_thread,
        },
        thread::Thread,
    },
    utils::userspace::user_ptr::UserPtr,
    vm::vmar::Vmar,
};

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

    let clear_child_tid = if (flags & CLONE_CHILD_CLEARTID) != 0 {
        child_tid_ptr
    } else {
        0
    };

    if (flags & CLONE_THREAD) != 0 {
        let child_thread = clone_thread(
            &caller_td,
            flags,
            stack,
            tls,
            clear_child_tid,
            Some(ctx),
        )?;
        let tid_val = child_thread.tid.as_u32() as i32;

        if (flags & CLONE_PARENT_SETTID) != 0 && parent_tid_ptr != 0 {
            write_user_i32(&caller_vmar, parent_tid_ptr, tid_val)?;
        }
        if (flags & CLONE_CHILD_SETTID) != 0 && child_tid_ptr != 0 {
            write_user_i32(&caller_vmar, child_tid_ptr, tid_val)?;
        }

        Ok(tid_val as usize)
    } else {
        let (child_proc, _child_thread, child_pid) = clone_proc(
            &caller_td,
            flags,
            stack,
            tls,
            clear_child_tid,
            Some(ctx),
        )?;
        let pid_val = child_pid.as_u32() as i32;

        if (flags & CLONE_PARENT_SETTID) != 0 && parent_tid_ptr != 0 {
            write_user_i32(&caller_vmar, parent_tid_ptr, pid_val)?;
        }
        if (flags & CLONE_CHILD_SETTID) != 0 && child_tid_ptr != 0 {
            write_user_i32(&child_proc.vmspace(), child_tid_ptr, pid_val)?;
        }

        Ok(pid_val as usize)
    }
}

/// System call `set_tid_address(int *tidptr)` (Syscall #218).
pub fn sys_set_tid_address(tidptr: usize, _ctx: &mut UserContext) -> Result<usize> {
    let td = Thread::current().ok_or(Errno::ESRCH)?;
    td.inner.lock().clear_child_tid = tidptr;
    Ok(td.tid.as_u32() as usize)
}
