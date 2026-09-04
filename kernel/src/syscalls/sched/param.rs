//! System calls for scheduling parameter inspection and modification.

use super::types::{resolve_target_thread, resolve_target_threads, SchedParam};
use crate::arch::syscall::SyscallFrame;
use crate::sched::policy::{RtPriority, SchedPolicy};
use crate::syscalls::{SyscallError, SyscallResult, UserPtr};

/// `sys_sched_getparam` (SYS_SCHED_GETPARAM = 143)
///
/// Retrieves scheduling parameters for the specified thread or process.
pub fn sys_sched_getparam(frame: &mut SyscallFrame) -> SyscallResult {
    let pid = frame.arg1() as i32;
    let param_ptr = UserPtr::<SchedParam>::from_u64(frame.arg2());

    let target = resolve_target_thread(pid)?;
    let th = target.lock();

    let priority = match th.sched_policy {
        SchedPolicy::Fair => 0,
        SchedPolicy::Fifo | SchedPolicy::RoundRobin => th.rt_priority.value() as i32,
    };

    let param = SchedParam {
        sched_priority: priority,
    };

    if !param_ptr.is_valid() {
        return Err(SyscallError::EFAULT);
    }

    param_ptr.write(param).ok_or(SyscallError::EFAULT)?;
    Ok(0)
}

/// `sys_sched_setparam` (SYS_SCHED_SETPARAM = 142)
///
/// Sets scheduling parameters for the specified thread or process.
pub fn sys_sched_setparam(frame: &mut SyscallFrame) -> SyscallResult {
    let pid = frame.arg1() as i32;
    let param_ptr = UserPtr::<SchedParam>::from_u64(frame.arg2());

    let param = param_ptr.read().ok_or(SyscallError::EFAULT)?;
    let targets = resolve_target_threads(pid)?;

    for thread in targets {
        let mut th = thread.lock();
        match th.sched_policy {
            SchedPolicy::Fair => {
                if param.sched_priority != 0 {
                    return Err(SyscallError::EINVAL);
                }
                th.rt_priority = RtPriority::DEFAULT;
            }
            SchedPolicy::Fifo | SchedPolicy::RoundRobin => {
                if param.sched_priority < 1 || param.sched_priority > 99 {
                    return Err(SyscallError::EINVAL);
                }
                let rt_prio = RtPriority::new(param.sched_priority as u8)
                    .map_err(|_| SyscallError::EINVAL)?;
                th.rt_priority = rt_prio;
            }
        }
    }

    Ok(0)
}
