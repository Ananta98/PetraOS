// SPDX-License-Identifier: GPL-2.0

//! Scheduler attributes system calls (`sched_setattr` and `sched_getattr`).

use ostd::arch::cpu::context::UserContext;

use super::param::resolve_target_proc;
use crate::api::errno::{Errno, Result};
use crate::api::sched::*;
use crate::proc::thread::Thread;
use crate::utils::userspace::UserPtr;

/// Maximum structure size accepted to prevent unbounded stack allocations.
const MAX_ATTR_SIZE: usize = 4096;

/// Handles `sched_setattr(2)` system call (`SYS_SCHED_SETATTR = 314`).
pub fn sys_sched_setattr(
    pid: i32,
    attr_ptr: usize,
    flags: u32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if flags != 0 {
        return Err(Errno::EINVAL);
    }
    let caller = Thread::proc_or_esrch()?;
    let vmar = caller.vmspace();

    // Read the size field first
    let mut size_buf = [0u8; 4];
    UserPtr::new(attr_ptr).read_bytes(&vmar, &mut size_buf)?;
    let size = u32::from_ne_bytes(size_buf) as usize;

    if size < SchedAttr::MIN_SIZE {
        return Err(Errno::EINVAL);
    }
    if size > MAX_ATTR_SIZE {
        return Err(Errno::E2BIG);
    }

    let read_len = size.min(SchedAttr::FULL_SIZE);
    let mut raw = [0u8; SchedAttr::FULL_SIZE];
    UserPtr::new(attr_ptr).read_bytes(&vmar, &mut raw[..read_len])?;

    let attr = SchedAttr::from_bytes(&raw[..read_len]).ok_or(Errno::EINVAL)?;

    let target = resolve_target_proc(&caller, pid)?;
    crate::sched::check_sched_permission(&caller, &target)?;

    let policy = attr.sched_policy as i32;
    match policy {
        SCHED_OTHER | SCHED_FIFO | SCHED_RR | SCHED_BATCH | SCHED_IDLE | SCHED_DEADLINE => {}
        _ => return Err(Errno::EINVAL),
    }

    let is_rt = matches!(policy, SCHED_FIFO | SCHED_RR | SCHED_DEADLINE);
    let reset_on_fork = (attr.sched_flags & SCHED_FLAG_RESET_ON_FORK) != 0;
    if (is_rt || reset_on_fork || attr.sched_nice < 0) && !caller.cred().is_superuser() {
        return Err(Errno::EPERM);
    }

    match policy {
        SCHED_OTHER | SCHED_BATCH => {
            if attr.sched_priority != 0 || !(-20..=19).contains(&attr.sched_nice) {
                return Err(Errno::EINVAL);
            }
        }
        SCHED_IDLE => {
            if attr.sched_priority != 0 {
                return Err(Errno::EINVAL);
            }
        }
        SCHED_FIFO | SCHED_RR => {
            if !(RT_PRIO_MIN as u32..=RT_PRIO_MAX as u32).contains(&attr.sched_priority) {
                return Err(Errno::EINVAL);
            }
        }
        SCHED_DEADLINE => {
            if attr.sched_priority != 0 {
                return Err(Errno::EINVAL);
            }
            if attr.sched_runtime == 0
                || attr.sched_deadline < attr.sched_runtime
                || attr.sched_period < attr.sched_deadline
            {
                return Err(Errno::EINVAL);
            }
        }
        _ => return Err(Errno::EINVAL),
    }

    crate::sched::update_sched_state(target.pid, |s| {
        s.policy = policy;
        s.flags = attr.sched_flags;
        s.nice = attr.sched_nice;
        s.priority = attr.sched_priority as i32;
        s.runtime = attr.sched_runtime;
        s.deadline = attr.sched_deadline;
        s.period = attr.sched_period;
    });
    crate::sched::apply_sched_state_to_proc(&target);

    Ok(0)
}

/// Handles `sched_getattr(2)` system call (`SYS_SCHED_GETATTR = 315`).
pub fn sys_sched_getattr(
    pid: i32,
    attr_ptr: usize,
    size: u32,
    flags: u32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if flags != 0 {
        return Err(Errno::EINVAL);
    }
    if attr_ptr == 0 {
        return Err(Errno::EFAULT);
    }
    if (size as usize) < SchedAttr::MIN_SIZE {
        return Err(Errno::EINVAL);
    }

    let caller = Thread::proc_or_esrch()?;
    let target = resolve_target_proc(&caller, pid)?;
    crate::sched::check_sched_permission(&caller, &target)?;

    let state = crate::sched::get_sched_state(target.pid);
    let attr = SchedAttr {
        size: SchedAttr::FULL_SIZE as u32,
        sched_policy: state.policy as u32,
        sched_flags: state.flags,
        sched_nice: state.nice,
        sched_priority: state.priority as u32,
        sched_runtime: state.runtime,
        sched_deadline: state.deadline,
        sched_period: state.period,
    };

    let full_bytes = attr.to_bytes();
    let write_len = (size as usize).min(SchedAttr::FULL_SIZE);
    let vmar = caller.vmspace();

    UserPtr::new(attr_ptr).write_bytes(&vmar, &full_bytes[..write_len])?;

    // If caller requested more than FULL_SIZE, zero out the rest
    if (size as usize) > SchedAttr::FULL_SIZE {
        let zeroes = [0u8; 128];
        let mut offset = SchedAttr::FULL_SIZE;
        let total = size as usize;
        while offset < total {
            let chunk = (total - offset).min(128);
            UserPtr::new(attr_ptr + offset).write_bytes(&vmar, &zeroes[..chunk])?;
            offset += chunk;
        }
    }

    Ok(0)
}
