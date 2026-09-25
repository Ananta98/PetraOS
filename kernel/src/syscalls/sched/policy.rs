//! System calls for scheduling policy inspection and modification.

use super::types::{
    resolve_target_thread, resolve_target_threads, SchedParam, SCHED_BATCH, SCHED_DEADLINE,
    SCHED_FIFO, SCHED_IDLE, SCHED_OTHER, SCHED_RESET_ON_FORK, SCHED_RR,
};
use crate::sched::policy::{RtPriority, SchedPolicy};
use crate::syscalls::{wrap_syscall, SyscallError, SyscallResult, UserPtr};

/// `sys_sched_getscheduler` (SYS_SCHED_GETSCHEDULER = 145)
///
/// Returns the current scheduling policy of the target thread/process.
#[wrap_syscall]
pub fn sys_sched_getscheduler(pid: i32) -> SyscallResult {
    let target = resolve_target_thread(pid)?;
    let policy = target.lock().sched_policy;

    let ret = match policy {
        SchedPolicy::Fair => SCHED_OTHER,
        SchedPolicy::Fifo => SCHED_FIFO,
        SchedPolicy::RoundRobin => SCHED_RR,
    };

    Ok(ret as usize)
}

/// `sys_sched_setscheduler` (SYS_SCHED_SETSCHEDULER = 144)
///
/// Sets the scheduling policy and priority for the target process / thread.
#[wrap_syscall]
pub fn sys_sched_setscheduler(
    pid: i32,
    raw_policy: u32,
    param_ptr: UserPtr<SchedParam>,
) -> SyscallResult {
    let param = param_ptr.read().ok_or(SyscallError::EFAULT)?;
    let clean_policy = raw_policy & !SCHED_RESET_ON_FORK;

    let (policy, rt_prio) = match clean_policy {
        SCHED_OTHER | SCHED_BATCH | SCHED_IDLE => {
            if param.sched_priority != 0 {
                return Err(SyscallError::EINVAL);
            }
            (SchedPolicy::Fair, RtPriority::DEFAULT)
        }
        SCHED_FIFO => {
            if param.sched_priority < 1 || param.sched_priority > 99 {
                return Err(SyscallError::EINVAL);
            }
            (
                SchedPolicy::Fifo,
                RtPriority::new(param.sched_priority as u8).map_err(|_| SyscallError::EINVAL)?,
            )
        }
        SCHED_RR => {
            if param.sched_priority < 1 || param.sched_priority > 99 {
                return Err(SyscallError::EINVAL);
            }
            (
                SchedPolicy::RoundRobin,
                RtPriority::new(param.sched_priority as u8).map_err(|_| SyscallError::EINVAL)?,
            )
        }
        SCHED_DEADLINE => {
            return Err(SyscallError::EINVAL);
        }
        _ => return Err(SyscallError::EINVAL),
    };

    let targets = resolve_target_threads(pid)?;
    for thread in targets {
        let mut th = thread.lock();
        th.sched_policy = policy;
        th.rt_priority = rt_prio;
    }

    Ok(0)
}

/// `sys_sched_get_priority_min` (SYS_SCHED_GET_PRIORITY_MIN = 147)
///
/// Returns the minimum priority value for a scheduling policy.
#[wrap_syscall]
pub fn sys_sched_get_priority_min(raw_policy: u32) -> SyscallResult {
    let clean_policy = raw_policy & !SCHED_RESET_ON_FORK;

    match clean_policy {
        SCHED_OTHER | SCHED_BATCH | SCHED_IDLE => Ok(0),
        SCHED_FIFO | SCHED_RR => Ok(1),
        _ => Err(SyscallError::EINVAL),
    }
}

/// `sys_sched_get_priority_max` (SYS_SCHED_GET_PRIORITY_MAX = 146)
///
/// Returns the maximum priority value for a scheduling policy.
#[wrap_syscall]
pub fn sys_sched_get_priority_max(raw_policy: u32) -> SyscallResult {
    let clean_policy = raw_policy & !SCHED_RESET_ON_FORK;

    match clean_policy {
        SCHED_OTHER | SCHED_BATCH | SCHED_IDLE => Ok(0),
        SCHED_FIFO | SCHED_RR => Ok(99),
        _ => Err(SyscallError::EINVAL),
    }
}
