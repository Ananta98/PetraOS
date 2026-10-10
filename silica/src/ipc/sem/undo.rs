// SPDX-License-Identifier: GPL-2.0

//! System V Semaphore Undo structures and process termination adjustments.

use alloc::{collections::BTreeMap, sync::Arc};
use ostd::sync::SpinLock;

use crate::{
    api::sem::{SEMAEM, SEMVMX},
    proc::{tree::Pid, Proc},
};

use super::set::SemSet;

/// Global registry of semaphore undo adjustments per process PID.
///
/// Maps `Pid -> ((semid, sem_num) -> adjustment)`.
static UNDO_REGISTRY: SpinLock<BTreeMap<Pid, BTreeMap<(i32, u16), i16>>> =
    SpinLock::new(BTreeMap::new());

/// Records or updates an undo adjustment for a specific process semaphore operation.
pub fn record_undo(pid: Pid, semid: i32, sem_num: u16, sem_op: i16) {
    let mut reg = UNDO_REGISTRY.lock();
    let proc_undos = reg.entry(pid).or_default();
    let adj = proc_undos.entry((semid, sem_num)).or_insert(0);

    // According to Linux ABI: adjustment is decremented by sem_op.
    let new_adj = (*adj as i32 - sem_op as i32).clamp(-SEMAEM as i32, SEMAEM as i32) as i16;
    if new_adj == 0 {
        proc_undos.remove(&(semid, sem_num));
        if proc_undos.is_empty() {
            reg.remove(&pid);
        }
    } else {
        *adj = new_adj;
    }
}

/// Clears undo entries corresponding to a semaphore set (or specific semaphore within it).
///
/// Invoked on `SETVAL`, `SETALL`, and `IPC_RMID`.
pub fn clear_undo_for_sem(semid: i32, sem_num: Option<u16>) {
    let mut reg = UNDO_REGISTRY.lock();
    let mut empty_pids = alloc::vec::Vec::new();

    for (&pid, proc_undos) in reg.iter_mut() {
        if let Some(num) = sem_num {
            proc_undos.remove(&(semid, num));
        } else {
            proc_undos.retain(|&(id, _), _| id != semid);
        }
        if proc_undos.is_empty() {
            empty_pids.push(pid);
        }
    }

    for pid in empty_pids {
        reg.remove(&pid);
    }
}

/// Applies all pending semaphore adjustments when a process terminates.
pub fn apply_proc_undo(
    proc: &Proc,
    now_secs: i64,
    lookup_set: impl Fn(i32) -> Option<Arc<SpinLock<SemSet>>>,
) {
    let undos = {
        let mut reg = UNDO_REGISTRY.lock();
        reg.remove(&proc.pid)
    };

    let Some(undos) = undos else {
        return;
    };

    for ((semid, sem_num), adj) in undos {
        if adj == 0 {
            continue;
        }

        if let Some(set_lock) = lookup_set(semid) {
            let mut set = set_lock.lock();
            if set.destroyed || (sem_num as usize) >= set.nsems {
                continue;
            }

            let sem = &mut set.sems[sem_num as usize];
            let new_val = (sem.val as i32 + adj as i32).clamp(0, SEMVMX as i32) as u16;
            sem.val = new_val;
            sem.pid = proc.pid.as_u32();
            set.ds.sem_otime = now_secs;
            set.wq.wake_all();
        }
    }
}
