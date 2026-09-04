//! ABI structures, constants, and helper functions for scheduler system calls.

use alloc::sync::Arc;
use alloc::vec::Vec;
use crate::proc::{all_processes, current_thread, find_process, ProcessId, Thread, ThreadId};
use crate::sync::Mutex;
use crate::syscalls::SyscallError;

/// POSIX / Linux `sched_param` structure.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SchedParam {
    /// Static scheduling priority.
    pub sched_priority: i32,
}

/// Linux `sched_attr` structure for `sched_setattr` / `sched_getattr`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SchedAttr {
    /// Size of this structure in bytes.
    pub size: u32,
    /// Policy (SCHED_OTHER, SCHED_FIFO, SCHED_RR, etc.).
    pub sched_policy: u32,
    /// Flags (`SCHED_FLAG_*`).
    pub sched_flags: u64,
    /// Nice value for SCHED_OTHER / SCHED_BATCH ([-20, 19]).
    pub sched_nice: i32,
    /// Static priority for real-time classes ([1, 99]).
    pub sched_priority: u32,
    /// Real-time runtime parameter (nanoseconds).
    pub sched_runtime: u64,
    /// Real-time deadline parameter (nanoseconds).
    pub sched_deadline: u64,
    /// Real-time period parameter (nanoseconds).
    pub sched_period: u64,
}

/// Standard Linux scheduling policy constants.
pub const SCHED_OTHER: u32 = 0;
pub const SCHED_FIFO: u32 = 1;
pub const SCHED_RR: u32 = 2;
pub const SCHED_BATCH: u32 = 3;
pub const SCHED_IDLE: u32 = 5;
pub const SCHED_DEADLINE: u32 = 6;

/// Flag indicating that policy/priority should reset on fork.
pub const SCHED_RESET_ON_FORK: u32 = 0x4000_0000;
pub const SCHED_FLAG_RESET_ON_FORK: u64 = 0x01;

/// Resolves a single target thread for a given PID/TID.
///
/// If `pid == 0`: returns the calling thread.
/// If `pid > 0`: searches for a process matching `pid`, returning its first thread.
/// If not found by process, searches across all processes for a thread matching `ThreadId(pid as u64)`.
pub fn resolve_target_thread(pid: i32) -> Result<Arc<Mutex<Thread>>, SyscallError> {
    if pid < 0 {
        return Err(SyscallError::EINVAL);
    }
    if pid == 0 {
        return current_thread().ok_or(SyscallError::ESRCH);
    }

    let target_id = pid as u64;
    if let Some(proc) = find_process(ProcessId::new(target_id)) {
        let p = proc.lock();
        if let Some(th) = p.threads.values().next() {
            return Ok(th.clone());
        }
    }

    // Try finding by ThreadId directly
    let target_tid = ThreadId(target_id);
    for proc in all_processes() {
        let p = proc.lock();
        if let Some(th) = p.threads.get(&target_tid) {
            return Ok(th.clone());
        }
    }

    Err(SyscallError::ESRCH)
}

/// Resolves all target threads associated with a given PID/TID.
///
/// If `pid == 0`: returns a vector containing the calling thread.
/// If `pid > 0`: searches for a process matching `pid`, returning all its threads.
/// If not found by process, searches for a specific thread matching `ThreadId(pid as u64)`.
pub fn resolve_target_threads(pid: i32) -> Result<Vec<Arc<Mutex<Thread>>>, SyscallError> {
    if pid < 0 {
        return Err(SyscallError::EINVAL);
    }
    if pid == 0 {
        let th = current_thread().ok_or(SyscallError::ESRCH)?;
        return Ok(alloc::vec![th]);
    }

    let target_id = pid as u64;
    if let Some(proc) = find_process(ProcessId::new(target_id)) {
        let p = proc.lock();
        let threads: Vec<Arc<Mutex<Thread>>> = p.threads.values().cloned().collect();
        if !threads.is_empty() {
            return Ok(threads);
        }
    }

    let target_tid = ThreadId(target_id);
    for proc in all_processes() {
        let p = proc.lock();
        if let Some(th) = p.threads.get(&target_tid) {
            return Ok(alloc::vec![th.clone()]);
        }
    }

    Err(SyscallError::ESRCH)
}
