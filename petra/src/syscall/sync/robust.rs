// SPDX-License-Identifier: GPL-2.0

//! Robust futex list system calls (`set_robust_list` #273, `get_robust_list` #274).

use alloc::sync::Arc;
use ostd::arch::cpu::context::UserContext;

use crate::{
    proc::{
        Tid, allproc_for_each,
        thread::Thread,
    },
    sync::robust::ROBUST_LIST_HEAD_LEN,
    uapi::errno::{Errno, Result},
    utils::UserPtr,
};

/// System call `set_robust_list(struct robust_list_head *head, size_t len)` (Syscall #273).
pub fn sys_set_robust_list(head: usize, len: usize, _ctx: &mut UserContext) -> Result<usize> {
    if len != ROBUST_LIST_HEAD_LEN {
        return Err(Errno::EINVAL);
    }

    let td = Thread::current().ok_or(Errno::ESRCH)?;
    let mut inner = td.inner.lock();
    inner.robust_list_head = head;
    inner.robust_list_len = len;

    Ok(0)
}

/// System call `get_robust_list(int pid, struct robust_list_head **head_ptr, size_t *len_ptr)` (Syscall #274).
pub fn sys_get_robust_list(
    pid: i32,
    head_ptr: usize,
    len_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if head_ptr == 0 || len_ptr == 0 {
        return Err(Errno::EFAULT);
    }

    let caller_td = Thread::current().ok_or(Errno::ESRCH)?;
    let caller_proc = caller_td.proc().ok_or(Errno::ESRCH)?;
    let vmar = caller_proc.vmspace();

    let target_td = if pid == 0 {
        caller_td.clone()
    } else {
        let target_tid = Tid::from_u32(pid as u32).ok_or(Errno::EINVAL)?;
        let mut found = None;
        allproc_for_each(|p| {
            if found.is_some() {
                return;
            }
            if let Some(t) = p.inner.lock().threads.iter().find(|t| t.tid == target_tid).cloned() {
                found = Some(t);
            }
        });
        found.ok_or(Errno::ESRCH)?
    };

    // Permission check: caller must be in same process or have appropriate privilege
    let target_proc = target_td.proc().ok_or(Errno::ESRCH)?;
    if !Arc::ptr_eq(&caller_proc, &target_proc) {
        let caller_cred = caller_proc.cred();
        let target_cred = target_proc.cred();
        if caller_cred.cr_uid.as_u32() != 0 && caller_cred.cr_uid != target_cred.cr_uid {
            return Err(Errno::EPERM);
        }
    }

    let (head, len) = {
        let inner = target_td.inner.lock();
        (inner.robust_list_head, inner.robust_list_len)
    };

    UserPtr::new(head_ptr).write_bytes(&vmar, &head.to_ne_bytes())?;
    UserPtr::new(len_ptr).write_bytes(&vmar, &len.to_ne_bytes())?;

    Ok(0)
}
