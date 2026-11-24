// SPDX-License-Identifier: GPL-2.0

//! Scheduler CPU affinity system calls.

use ostd::arch::cpu::context::UserContext;

use super::param::resolve_target_proc;
use crate::uapi::errno::{Errno, Result};
use crate::uapi::sched::{CPU_BYTES, CpuMask};
use crate::proc::thread::Thread;
use crate::utils::userspace::UserPtr;

/// Minimum allowed CPU set size in bytes (size of an unsigned long).
const MIN_CPUSET_SIZE: usize = 8;

/// Handles `sched_setaffinity(2)` system call (`SYS_SCHED_SETAFFINITY = 203`).
pub fn sys_sched_setaffinity(
    pid: i32,
    cpusetsize: usize,
    mask_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if cpusetsize < MIN_CPUSET_SIZE {
        return Err(Errno::EINVAL);
    }
    let caller = Thread::proc_or_esrch()?;
    let vmar = caller.vmspace();

    // Linux ignores bits beyond nr_cpu_ids / CPU_BYTES; avoid looping over an
    // unbounded user-supplied cpusetsize.
    let read_len = cpusetsize.min(CPU_BYTES);
    let mut raw_buf = [0u8; CPU_BYTES];
    UserPtr::new(mask_ptr).read_bytes(&vmar, &mut raw_buf[..read_len])?;

    let mask = CpuMask::from_slice(&raw_buf[..read_len]);
    if mask.is_empty() {
        return Err(Errno::EINVAL);
    }

    let target = resolve_target_proc(&caller, pid)?;
    crate::sched::check_sched_permission(&caller, &target)?;

    crate::sched::update_sched_state(target.pid, |s| {
        s.affinity = mask;
    });
    crate::sched::apply_sched_state_to_proc(&target);

    Ok(0)
}

/// Handles `sched_getaffinity(2)` system call (`SYS_SCHED_GETAFFINITY = 204`).
pub fn sys_sched_getaffinity(
    pid: i32,
    cpusetsize: usize,
    mask_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if cpusetsize < MIN_CPUSET_SIZE || !cpusetsize.is_multiple_of(MIN_CPUSET_SIZE) {
        return Err(Errno::EINVAL);
    }
    let caller = Thread::proc_or_esrch()?;
    let target = resolve_target_proc(&caller, pid)?;

    let state = crate::sched::get_sched_state(target.pid);
    let full_mask = state.affinity.to_bytes();
    let write_len = cpusetsize.min(CPU_BYTES);

    let vmar = caller.vmspace();
    UserPtr::new(mask_ptr).write_bytes(&vmar, &full_mask[..write_len])?;

    Ok(write_len)
}
