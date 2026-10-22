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

    let read_len = cpusetsize.min(CPU_BYTES);
    let mut raw_buf = [0u8; CPU_BYTES];
    UserPtr::new(mask_ptr).read_bytes(&vmar, &mut raw_buf[..read_len])?;

    // If caller provided a buffer larger than 128 bytes, ensure extra bytes are zero.
    if cpusetsize > CPU_BYTES {
        let mut extra = [0u8; 128];
        let mut offset = CPU_BYTES;
        while offset < cpusetsize {
            let chunk = (cpusetsize - offset).min(128);
            UserPtr::new(mask_ptr + offset).read_bytes(&vmar, &mut extra[..chunk])?;
            if extra[..chunk].iter().any(|&b| b != 0) {
                return Err(Errno::EINVAL);
            }
            offset += chunk;
        }
    }

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
    if cpusetsize < MIN_CPUSET_SIZE {
        return Err(Errno::EINVAL);
    }
    let caller = Thread::proc_or_esrch()?;
    let target = resolve_target_proc(&caller, pid)?;
    crate::sched::check_sched_permission(&caller, &target)?;

    let state = crate::sched::get_sched_state(target.pid);
    let full_mask = state.affinity.to_bytes();
    let write_len = cpusetsize.min(CPU_BYTES);

    let vmar = caller.vmspace();
    UserPtr::new(mask_ptr).write_bytes(&vmar, &full_mask[..write_len])?;

    // If caller's buffer is larger than CPU_BYTES, zero out the rest
    if cpusetsize > CPU_BYTES {
        let zeroes = [0u8; 128];
        let mut offset = CPU_BYTES;
        while offset < cpusetsize {
            let chunk = (cpusetsize - offset).min(128);
            UserPtr::new(mask_ptr + offset).write_bytes(&vmar, &zeroes[..chunk])?;
            offset += chunk;
        }
    }

    Ok(write_len)
}
