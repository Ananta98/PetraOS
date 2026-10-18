// SPDX-License-Identifier: GPL-2.0

//! `membarrier` system call (Syscall #324 on x86_64).
//!
//! User space uses `membarrier(2)` to force memory barriers on the CPUs that
//! can observe its shared memory. The kernel tracks which commands each process
//! has registered for and, when a barrier is requested, interrupts every CPU so
//! that the fence is executed where it is needed.

use core::sync::atomic::{Ordering, fence};

use ostd::{arch::cpu::context::UserContext, cpu::CpuSet, smp::inter_processor_call};

use crate::{
    api::errno::{Errno, Result},
    proc::{Proc, thread::Thread},
};

/// Ask which membarrier commands the kernel supports.
const MEMBARRIER_CMD_QUERY: i32 = 1 << 0;
/// Issue a global memory barrier without requiring registration.
const MEMBARRIER_CMD_GLOBAL: i32 = 1 << 1;
/// Issue a global expedited barrier to registered processes.
const MEMBARRIER_CMD_GLOBAL_EXPEDITED: i32 = 1 << 2;
/// Register the caller for `MEMBARRIER_CMD_GLOBAL_EXPEDITED`.
const MEMBARRIER_CMD_REGISTER_GLOBAL_EXPEDITED: i32 = 1 << 3;
/// Issue a private expedited barrier to the caller's threads.
const MEMBARRIER_CMD_PRIVATE_EXPEDITED: i32 = 1 << 4;
/// Register the caller for `MEMBARRIER_CMD_PRIVATE_EXPEDITED`.
const MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED: i32 = 1 << 5;
/// Issue a private expedited barrier that also synchronizes the instruction stream.
const MEMBARRIER_CMD_PRIVATE_EXPEDITED_SYNC_CORE: i32 = 1 << 6;
/// Register the caller for `MEMBARRIER_CMD_PRIVATE_EXPEDITED_SYNC_CORE`.
const MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED_SYNC_CORE: i32 = 1 << 7;

/// The commands reported by `MEMBARRIER_CMD_QUERY`.
const SUPPORTED_COMMANDS: i32 = MEMBARRIER_CMD_GLOBAL
    | MEMBARRIER_CMD_GLOBAL_EXPEDITED
    | MEMBARRIER_CMD_REGISTER_GLOBAL_EXPEDITED
    | MEMBARRIER_CMD_PRIVATE_EXPEDITED
    | MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED
    | MEMBARRIER_CMD_PRIVATE_EXPEDITED_SYNC_CORE
    | MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED_SYNC_CORE;

/// `membarrier(2)` - Issues a memory barrier as requested by user space.
pub fn sys_membarrier(
    cmd: i32,
    flags: u32,
    _cpu_id: i32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if flags != 0 {
        return Err(Errno::EINVAL);
    }

    if cmd == MEMBARRIER_CMD_QUERY {
        return Ok(SUPPORTED_COMMANDS as usize);
    }

    if cmd.count_ones() != 1 || cmd & !SUPPORTED_COMMANDS != 0 {
        return Err(Errno::EINVAL);
    }

    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;

    match cmd {
        MEMBARRIER_CMD_GLOBAL => {
            issue_memory_barrier();
        }
        MEMBARRIER_CMD_REGISTER_GLOBAL_EXPEDITED => {
            register(&proc, MEMBARRIER_CMD_GLOBAL_EXPEDITED);
        }
        MEMBARRIER_CMD_GLOBAL_EXPEDITED => {
            require_registered(&proc, MEMBARRIER_CMD_GLOBAL_EXPEDITED)?;
            issue_memory_barrier();
        }
        MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED => {
            register(&proc, MEMBARRIER_CMD_PRIVATE_EXPEDITED);
        }
        MEMBARRIER_CMD_PRIVATE_EXPEDITED => {
            require_registered(&proc, MEMBARRIER_CMD_PRIVATE_EXPEDITED)?;
            issue_memory_barrier();
        }
        MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED_SYNC_CORE => {
            // Registering for the sync-core variant also covers the plain
            // private expedited command, matching Linux.
            register(
                &proc,
                MEMBARRIER_CMD_PRIVATE_EXPEDITED_SYNC_CORE | MEMBARRIER_CMD_PRIVATE_EXPEDITED,
            );
        }
        MEMBARRIER_CMD_PRIVATE_EXPEDITED_SYNC_CORE => {
            require_registered(&proc, MEMBARRIER_CMD_PRIVATE_EXPEDITED_SYNC_CORE)?;
            issue_memory_barrier();
        }
        _ => return Err(Errno::EINVAL),
    }

    Ok(0)
}

/// Records `cmd` as registered for `proc`.
fn register(proc: &Proc, cmd: i32) {
    proc.membarrier_state
        .fetch_or(cmd as u32, Ordering::Release);
}

/// Rejects the request with `EPERM` unless `proc` registered for `cmd`.
fn require_registered(proc: &Proc, cmd: i32) -> Result<()> {
    if proc.membarrier_state.load(Ordering::Acquire) & cmd as u32 == 0 {
        return Err(Errno::EPERM);
    }
    Ok(())
}

/// Runs a sequentially consistent fence on every CPU in the system and waits
/// for the remote processors to finish. The current CPU runs the fence inline,
/// the others run it from an IPI handler.
fn issue_memory_barrier() {
    fn fence_here() {
        fence(Ordering::SeqCst);
    }

    inter_processor_call(&CpuSet::new_full(), fence_here).wait();
}
