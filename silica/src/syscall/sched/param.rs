// SPDX-License-Identifier: GPL-2.0

//! Scheduler parameter and policy system calls.

use alloc::sync::Arc;
use ostd::arch::cpu::context::UserContext;

use crate::api::errno::{Errno, Result};
use crate::api::sched::*;
use crate::proc::Proc;
use crate::proc::thread::Thread;
use crate::proc::tree::{Pid, allproc_find};
use crate::utils::userspace::UserPtr;

/// Resolves target process for scheduler syscalls.
pub fn resolve_target_proc(caller: &Arc<Proc>, pid_val: i32) -> Result<Arc<Proc>> {
    if pid_val == 0 {
        Ok(caller.clone())
    } else if pid_val > 0 {
        let pid = Pid::from_u32(pid_val as u32).ok_or(Errno::ESRCH)?;
        allproc_find(pid).ok_or(Errno::ESRCH)
    } else {
        Err(Errno::EINVAL)
    }
}

/// Handles `sched_setparam(2)` system call (`SYS_SCHED_SETPARAM = 142`).
pub fn sys_sched_setparam(
    pid: i32,
    param_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if param_ptr == 0 {
        return Err(Errno::EFAULT);
    }

    let caller = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let vmar = caller.vmspace();

    let mut buf = [0u8; 4];
    UserPtr::new(param_ptr).read_bytes(&vmar, &mut buf)?;
    let param = SchedParam::from_bytes(&buf).ok_or(Errno::EINVAL)?;

    let target = resolve_target_proc(&caller, pid)?;
    crate::sched::check_sched_permission(&caller, &target)?;

    let current_state = crate::sched::get_sched_state(target.pid);
    match current_state.policy {
        SCHED_OTHER | SCHED_BATCH | SCHED_IDLE | SCHED_DEADLINE => {
            if param.sched_priority != 0 {
                return Err(Errno::EINVAL);
            }
        }
        SCHED_FIFO | SCHED_RR => {
            if !(RT_PRIO_MIN..=RT_PRIO_MAX).contains(&param.sched_priority) {
                return Err(Errno::EINVAL);
            }
        }
        _ => return Err(Errno::EINVAL),
    }

    crate::sched::update_sched_state(target.pid, |s| {
        s.priority = param.sched_priority;
    });
    crate::sched::apply_sched_state_to_proc(&target);

    Ok(0)
}

/// Handles `sched_getparam(2)` system call (`SYS_SCHED_GETPARAM = 143`).
pub fn sys_sched_getparam(
    pid: i32,
    param_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if param_ptr == 0 {
        return Err(Errno::EFAULT);
    }

    let caller = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let target = resolve_target_proc(&caller, pid)?;
    crate::sched::check_sched_permission(&caller, &target)?;

    let state = crate::sched::get_sched_state(target.pid);
    let param = SchedParam::new(state.priority);

    UserPtr::new(param_ptr).write_bytes(&caller.vmspace(), &param.to_bytes())?;
    Ok(0)
}

/// Handles `sched_setscheduler(2)` system call (`SYS_SCHED_SETSCHEDULER = 144`).
pub fn sys_sched_setscheduler(
    pid: i32,
    policy: i32,
    param_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if param_ptr == 0 {
        return Err(Errno::EFAULT);
    }

    let reset_on_fork = (policy & SCHED_RESET_ON_FORK) != 0;
    let base_policy = policy & !SCHED_RESET_ON_FORK;

    match base_policy {
        SCHED_OTHER | SCHED_FIFO | SCHED_RR | SCHED_BATCH | SCHED_IDLE | SCHED_DEADLINE => {}
        _ => return Err(Errno::EINVAL),
    }

    let caller = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let vmar = caller.vmspace();

    let mut buf = [0u8; 4];
    UserPtr::new(param_ptr).read_bytes(&vmar, &mut buf)?;
    let param = SchedParam::from_bytes(&buf).ok_or(Errno::EINVAL)?;

    let target = resolve_target_proc(&caller, pid)?;
    crate::sched::check_sched_permission(&caller, &target)?;

    let is_rt = matches!(base_policy, SCHED_FIFO | SCHED_RR | SCHED_DEADLINE);
    if (is_rt || reset_on_fork) && !caller.cred().is_superuser() {
        return Err(Errno::EPERM);
    }

    match base_policy {
        SCHED_OTHER | SCHED_BATCH | SCHED_IDLE | SCHED_DEADLINE => {
            if param.sched_priority != 0 {
                return Err(Errno::EINVAL);
            }
        }
        SCHED_FIFO | SCHED_RR => {
            if !(RT_PRIO_MIN..=RT_PRIO_MAX).contains(&param.sched_priority) {
                return Err(Errno::EINVAL);
            }
        }
        _ => return Err(Errno::EINVAL),
    }

    crate::sched::update_sched_state(target.pid, |s| {
        s.policy = base_policy;
        s.priority = param.sched_priority;
        if reset_on_fork {
            s.flags |= SCHED_FLAG_RESET_ON_FORK;
        }
    });
    crate::sched::apply_sched_state_to_proc(&target);

    Ok(0)
}

/// Handles `sched_getscheduler(2)` system call (`SYS_SCHED_GETSCHEDULER = 145`).
pub fn sys_sched_getscheduler(pid: i32, _ctx: &mut UserContext) -> Result<usize> {
    let caller = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let target = resolve_target_proc(&caller, pid)?;
    crate::sched::check_sched_permission(&caller, &target)?;

    let state = crate::sched::get_sched_state(target.pid);
    Ok(state.policy as usize)
}

/// Handles `sched_get_priority_max(2)` system call (`SYS_SCHED_GET_PRIORITY_MAX = 146`).
pub fn sys_sched_get_priority_max(policy: i32, _ctx: &mut UserContext) -> Result<usize> {
    match policy {
        SCHED_FIFO | SCHED_RR => Ok(RT_PRIO_MAX as usize),
        SCHED_OTHER | SCHED_BATCH | SCHED_IDLE | SCHED_DEADLINE => Ok(0),
        _ => Err(Errno::EINVAL),
    }
}

/// Handles `sched_get_priority_min(2)` system call (`SYS_SCHED_GET_PRIORITY_MIN = 147`).
pub fn sys_sched_get_priority_min(policy: i32, _ctx: &mut UserContext) -> Result<usize> {
    match policy {
        SCHED_FIFO | SCHED_RR => Ok(RT_PRIO_MIN as usize),
        SCHED_OTHER | SCHED_BATCH | SCHED_IDLE | SCHED_DEADLINE => Ok(0),
        _ => Err(Errno::EINVAL),
    }
}
