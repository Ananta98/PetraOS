// SPDX-License-Identifier: GPL-2.0

//! Process and thread scheduling attributes and CPU affinity state.

use alloc::collections::BTreeMap;
use ostd::sync::SpinLock;

use crate::api::errno::{Errno, Result};
use crate::api::sched::{CpuMask, SCHED_OTHER};
use crate::proc::{Pid, Proc};

/// Per-process scheduling attributes and CPU affinity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcSchedState {
    pub policy: i32,
    pub priority: i32,
    pub nice: i32,
    pub flags: u64,
    pub runtime: u64,
    pub deadline: u64,
    pub period: u64,
    pub affinity: CpuMask,
}

impl Default for ProcSchedState {
    fn default() -> Self {
        Self {
            policy: SCHED_OTHER,
            priority: 0,
            nice: 0,
            flags: 0,
            runtime: 0,
            deadline: 0,
            period: 0,
            affinity: CpuMask::all(),
        }
    }
}

/// Global registry mapping process PID to scheduling state.
static SCHED_REGISTRY: SpinLock<BTreeMap<Pid, ProcSchedState>> = SpinLock::new(BTreeMap::new());

/// Gets the current scheduling state for a process. Returns default if unconfigured.
pub fn get_sched_state(pid: Pid) -> ProcSchedState {
    let registry = SCHED_REGISTRY.lock();
    registry.get(&pid).copied().unwrap_or_default()
}

/// Updates the scheduling state for a process.
pub fn update_sched_state<F>(pid: Pid, f: F)
where
    F: FnOnce(&mut ProcSchedState),
{
    let mut registry = SCHED_REGISTRY.lock();
    let state = registry.entry(pid).or_default();
    f(state);
}

/// Cleans up scheduling state when a process terminates.
pub fn on_proc_exit(pid: Pid) {
    let mut registry = SCHED_REGISTRY.lock();
    registry.remove(&pid);
}

/// Applies updated scheduling parameters across all threads belonging to `proc`.
pub fn apply_sched_state_to_proc(proc: &Proc) {
    let state = get_sched_state(proc.pid);
    let threads = proc.inner.lock().threads.clone();
    for td in threads {
        if let Some(entity) = crate::sched::entity_of(&td.td_kstack) {
            entity.set_params(state.policy, state.priority, state.nice as i8);
            entity.set_affinity(state.affinity);
        }
    }
}

/// Validates permission of `caller` to inspect or modify `target`'s scheduler parameters.
pub fn check_sched_permission(caller: &Proc, target: &Proc) -> Result<()> {
    if caller.pid == target.pid {
        return Ok(());
    }
    let caller_cred = caller.cred();
    if caller_cred.is_superuser() {
        return Ok(());
    }
    let target_cred = target.cred();
    if caller_cred.cr_uid == target_cred.cr_uid
        || caller_cred.cr_ruid == target_cred.cr_uid
        || caller_cred.cr_uid == target_cred.cr_ruid
        || caller_cred.cr_ruid == target_cred.cr_ruid
    {
        return Ok(());
    }
    Err(Errno::EPERM)
}
