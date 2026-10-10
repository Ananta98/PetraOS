// SPDX-License-Identifier: GPL-2.0

//! The `prlimit64` system call (Syscall #302 on x86_64).

use alloc::sync::Arc;
use ostd::arch::cpu::context::UserContext;

use crate::{
    api::{
        errno::{Errno, Result},
        limit::{RLimit, Resource},
    },
    proc::{
        thread::Thread,
        tree::{Pid, allproc_find},
    },
    utils::userspace::user_ptr::UserPtr,
};

fn linux_to_resource(res: u32) -> Option<Resource> {
    match res {
        0 => Some(Resource::Cpu),
        1 => Some(Resource::Fsize),
        2 => Some(Resource::Data),
        3 => Some(Resource::Stack),
        4 => Some(Resource::Core),
        5 => Some(Resource::Rss),
        6 => Some(Resource::Nproc),
        7 => Some(Resource::Nofile),
        8 => Some(Resource::Memlock),
        9 => Some(Resource::As),
        _ => None,
    }
}

/// System call `prlimit64(pid, resource, new_limit, old_limit)` (Syscall #302).
pub fn sys_prlimit64(
    pid: i32,
    resource: u32,
    new_limit_ptr: usize,
    old_limit_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let caller_td = Thread::current().ok_or(Errno::ESRCH)?;
    let caller_proc = caller_td.proc().ok_or(Errno::ESRCH)?;

    let target_proc = if pid == 0 {
        Arc::clone(&caller_proc)
    } else {
        if pid < 0 {
            crate::return_errno!(EINVAL, "negative pid in prlimit64");
        }
        let target_pid = Pid::from_u32(pid as u32).ok_or(Errno::ESRCH)?;
        allproc_find(target_pid).ok_or(Errno::ESRCH)?
    };

    let caller_vmar = caller_proc.vmspace();

    // 1. If old_limit_ptr != 0, write current limit
    if old_limit_ptr != 0 {
        let rlim = match linux_to_resource(resource) {
            Some(res) => target_proc.limit().get(res),
            None => RLimit::infinity(),
        };

        let mut buf = [0u8; 16];
        buf[0..8].copy_from_slice(&rlim.rlim_cur.to_ne_bytes());
        buf[8..16].copy_from_slice(&rlim.rlim_max.to_ne_bytes());

        UserPtr::new(old_limit_ptr).write_bytes(&caller_vmar, &buf)?;
    }

    // 2. If new_limit_ptr != 0, set new limit
    if new_limit_ptr != 0 {
        let mut buf = [0u8; 16];
        UserPtr::new(new_limit_ptr).read_bytes(&caller_vmar, &mut buf)?;

        let cur = u64::from_ne_bytes([
            buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7],
        ]);
        let max = u64::from_ne_bytes([
            buf[8], buf[9], buf[10], buf[11], buf[12], buf[13], buf[14], buf[15],
        ]);

        let new_rlim = RLimit::new(cur, max);

        if let Some(res) = linux_to_resource(resource) {
            let mut current_limits = (*target_proc.limit()).clone();
            let is_root = caller_proc.cred().cr_uid.is_root();
            current_limits.set(res, new_rlim, is_root)?;
            target_proc.set_limit(Arc::new(current_limits));
        }
    }

    Ok(0)
}

/// System call `getrlimit(resource, rlim)` (Syscall #97).
pub fn sys_getrlimit(resource: u32, rlim_ptr: usize, ctx: &mut UserContext) -> Result<usize> {
    sys_prlimit64(0, resource, 0, rlim_ptr, ctx)
}

/// System call `setrlimit(resource, rlim)` (Syscall #160).
pub fn sys_setrlimit(resource: u32, rlim_ptr: usize, ctx: &mut UserContext) -> Result<usize> {
    sys_prlimit64(0, resource, rlim_ptr, 0, ctx)
}
