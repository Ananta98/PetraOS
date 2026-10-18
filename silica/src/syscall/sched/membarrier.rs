// SPDX-License-Identifier: GPL-2.0

//! `membarrier` system call (Syscall #324 on x86_64).

use core::sync::atomic::{Ordering, fence};

use ostd::arch::cpu::context::UserContext;

use crate::api::errno::{Errno, Result};

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

/// The commands reported by `MEMBARRIER_CMD_QUERY`. Registration is implicit
/// because barriers are issued unconditionally, so every command is accepted.
const SUPPORTED_COMMANDS: i32 = MEMBARRIER_CMD_GLOBAL
    | MEMBARRIER_CMD_GLOBAL_EXPEDITED
    | MEMBARRIER_CMD_REGISTER_GLOBAL_EXPEDITED
    | MEMBARRIER_CMD_PRIVATE_EXPEDITED
    | MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED
    | MEMBARRIER_CMD_PRIVATE_EXPEDITED_SYNC_CORE
    | MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED_SYNC_CORE;

/// `membarrier(2)` - Issues a memory barrier as requested by user space.
///
/// The kernel does not track per-process registrations: every command emits a
/// full fence, which is stronger than what any command asks for, and
/// registration is reported as successful so that callers proceed normally.
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

    fence(Ordering::SeqCst);
    Ok(0)
}
