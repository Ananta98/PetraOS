// SPDX-License-Identifier: GPL-2.0

//! System V Semaphore subsystem.
//!
//! Provides semaphore set creation, atomic array operations (`semop`, `semtimedop`),
//! control commands (`semctl`), and process undo cleanup on termination.

pub mod set;
pub mod undo;

use alloc::{sync::Arc, vec::Vec};
use core::time::Duration;
use ostd::sync::{SpinLock, Waiter};

use crate::{
    ipc::perm::{check_admin_permission, check_read_permission, check_write_permission},
    ipc::registry::IpcRegistry,
    proc::{Proc, thread::Thread},
    uapi::{
        cred::Ucred,
        errno::{Errno, Result},
        ipc::{
            IPC_CREAT, IPC_EXCL, IPC_INFO, IPC_NOWAIT, IPC_PRIVATE, IPC_RMID, IPC_SET, IPC_STAT,
            IpcPerm,
        },
        sem::{
            GETALL, GETNCNT, GETPID, GETVAL, GETZCNT, SEM_INFO, SEM_STAT, SEM_STAT_ANY, SEM_UNDO,
            SEMMNI, SEMMSL, SEMOPM, SEMVMX, SETALL, SETVAL, SemBuf, SemIdDs, SemInfo,
        },
    },
    utils::userspace::user_ptr::UserPtr,
};

pub use self::{
    set::{SemSet, Semaphore},
    undo::{clear_undo_for_sem, record_undo},
};

/// Global table of active semaphore sets by `semid`.
static SEM_REGISTRY: IpcRegistry<SpinLock<SemSet>> = IpcRegistry::new();

/// Returns current monotonic uptime in seconds.
fn current_time_secs() -> i64 {
    crate::time::uptime_secs() as i64
}

/// Looks up an active semaphore set by `semid`.
pub fn lookup_set(semid: i32) -> Option<Arc<SpinLock<SemSet>>> {
    SEM_REGISTRY.get(semid)
}

/// `semget(2)` - Obtains or creates a System V semaphore set.
pub fn sem_get(key: i32, nsems: i32, semflg: i32, cred: &Ucred) -> Result<i32> {
    if nsems < 0 || (nsems as usize) > SEMMSL {
        return Err(Errno::EINVAL);
    }
    let nsems = nsems as usize;

    if key == IPC_PRIVATE {
        if nsems == 0 {
            return Err(Errno::EINVAL);
        }
        return create_sem_set(IPC_PRIVATE, nsems, semflg, cred);
    }

    // Check if key already exists
    let existing_semid = SEM_REGISTRY.lookup_key(key);
    if let Some(semid) = existing_semid {
        if (semflg & IPC_CREAT != 0) && (semflg & IPC_EXCL != 0) {
            return Err(Errno::EEXIST);
        }

        let set_lock = lookup_set(semid).ok_or(Errno::ENOENT)?;
        let set = set_lock.lock();
        if set.destroyed {
            return Err(Errno::EIDRM);
        }

        check_read_permission(&set.ds.sem_perm, cred)?;
        if nsems > 0 && nsems > set.nsems {
            return Err(Errno::EINVAL);
        }

        return Ok(semid);
    }

    // Key does not exist
    if semflg & IPC_CREAT == 0 {
        return Err(Errno::ENOENT);
    }
    if nsems == 0 {
        return Err(Errno::EINVAL);
    }

    create_sem_set(key, nsems, semflg, cred)
}

/// Creates a new semaphore set and registers it globally.
fn create_sem_set(key: i32, nsems: usize, semflg: i32, cred: &Ucred) -> Result<i32> {
    let mut reg = SEM_REGISTRY.lock();
    if reg.len() >= SEMMNI {
        return Err(Errno::ENOSPC);
    }

    let semid = crate::ipc::id::alloc_ipc_id(&reg);
    let now = current_time_secs();

    let ds = SemIdDs {
        sem_perm: IpcPerm::new(key, cred, semflg as u32),
        sem_otime: 0,
        glibc_reserved1: 0,
        sem_ctime: now,
        glibc_reserved2: 0,
        sem_nsems: nsems,
        glibc_reserved3: 0,
        glibc_reserved4: 0,
    };

    let set = SemSet::new(semid, key, nsems, ds);
    reg.insert(semid, Arc::clone(&set));
    SEM_REGISTRY.bind_key(key, semid);

    Ok(semid)
}

/// Tests whether all operations in `sops` can execute atomically right now.
fn can_execute_ops(set: &SemSet, sops: &[SemBuf]) -> Result<bool> {
    for op in sops {
        let sem = &set.sems[op.sem_num as usize];
        if op.sem_op > 0 {
            if (sem.val as i32 + op.sem_op as i32) > (SEMVMX as i32) {
                return Err(Errno::ERANGE);
            }
        } else if op.sem_op == 0 {
            if sem.val != 0 {
                return Ok(false);
            }
        } else {
            let req = (-op.sem_op) as u16;
            if sem.val < req {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// Applies all operations in `sops` atomically to `set`.
fn apply_ops(set: &mut SemSet, sops: &[SemBuf], proc: &Proc) {
    let now = current_time_secs();
    for op in sops {
        let sem = &mut set.sems[op.sem_num as usize];
        if op.sem_op > 0 {
            sem.val += op.sem_op as u16;
        } else if op.sem_op < 0 {
            sem.val -= (-op.sem_op) as u16;
        }
        sem.pid = proc.pid.as_u32();

        if (op.sem_flg & SEM_UNDO) != 0 {
            record_undo(proc.pid, set.semid, op.sem_num, op.sem_op);
        }
    }
    set.ds.sem_otime = now;
}

/// `semop(2)` / `semtimedop(2)` - Performs atomic semaphore operations on a set.
pub fn sem_op(
    semid: i32,
    sops_ptr: usize,
    nsops: usize,
    timeout: Option<Duration>,
    cred: &Ucred,
    proc: &Arc<Proc>,
) -> Result<usize> {
    if nsops == 0 || nsops > SEMOPM {
        return Err(Errno::E2BIG);
    }
    if sops_ptr == 0 {
        return Err(Errno::EFAULT);
    }

    let vmar = proc.vmspace();
    let mut sops = Vec::with_capacity(nsops);
    let mut raw = [0u8; 6];

    for i in 0..nsops {
        let elem_ptr = sops_ptr + i * 6;
        UserPtr::new(elem_ptr).read_bytes(&vmar, &mut raw)?;
        let buf = SemBuf::from_bytes(&raw).ok_or(Errno::EINVAL)?;
        sops.push(buf);
    }

    let set_lock = lookup_set(semid).ok_or(Errno::EINVAL)?;

    // Initial permission and bounds verification
    {
        let set = set_lock.lock();
        if set.destroyed {
            return Err(Errno::EIDRM);
        }
        for op in &sops {
            if (op.sem_num as usize) >= set.nsems {
                return Err(Errno::EFBIG);
            }
        }
        let alter = sops.iter().any(|op| op.sem_op != 0);
        if alter {
            check_write_permission(&set.ds.sem_perm, cred)?;
        } else {
            check_read_permission(&set.ds.sem_perm, cred)?;
        }
    }

    // Fast path: try to execute operations immediately
    {
        let mut set = set_lock.lock();
        if set.destroyed {
            return Err(Errno::EIDRM);
        }
        if can_execute_ops(&set, &sops)? {
            apply_ops(&mut set, &sops, proc);
            set.wq.wake_all();
            return Ok(0);
        }
    }

    let has_nowait = sops
        .iter()
        .any(|op| (op.sem_flg & (IPC_NOWAIT as i16)) != 0);
    if has_nowait || timeout == Some(Duration::ZERO) {
        return Err(Errno::EAGAIN);
    }

    // Slow path: block on wait queue until conditions are satisfied or cancelled
    let deadline = timeout.map(|d| ostd::timer::Jiffies::elapsed().as_duration() + d);

    // Increment ncnt/zcnt while waiting
    {
        let mut set = set_lock.lock();
        for op in &sops {
            if op.sem_op < 0 {
                set.sems[op.sem_num as usize].ncnt =
                    set.sems[op.sem_num as usize].ncnt.saturating_add(1);
            } else if op.sem_op == 0 {
                set.sems[op.sem_num as usize].zcnt =
                    set.sems[op.sem_num as usize].zcnt.saturating_add(1);
            }
        }
    }

    let (waiter, _) = Waiter::new_pair();

    let cond = || {
        let mut set = set_lock.lock();
        if set.destroyed {
            return Some(Err(Errno::EIDRM));
        }

        match can_execute_ops(&set, &sops) {
            Ok(true) => {
                apply_ops(&mut set, &sops, proc);
                set.wq.wake_all();
                Some(Ok(()))
            }
            Ok(false) => {
                set.wq.enqueue(waiter.waker());
                None
            }
            Err(e) => Some(Err(e)),
        }
    };

    let cancel_cond = || {
        if let Some(dl) = deadline {
            if ostd::timer::Jiffies::elapsed().as_duration() >= dl {
                return Err(Errno::EAGAIN);
            }
        }
        if Thread::should_interrupt() {
            return Err(Errno::EINTR);
        }
        Ok(())
    };

    let wait_res = waiter.wait_until_or_cancelled(cond, cancel_cond);

    // Clean up ncnt/zcnt counts on completion or abort
    {
        let mut set = set_lock.lock();
        for op in &sops {
            if op.sem_op < 0 {
                set.sems[op.sem_num as usize].ncnt =
                    set.sems[op.sem_num as usize].ncnt.saturating_sub(1);
            } else if op.sem_op == 0 {
                set.sems[op.sem_num as usize].zcnt =
                    set.sems[op.sem_num as usize].zcnt.saturating_sub(1);
            }
        }
    }

    match wait_res {
        Ok(inner_res) => {
            inner_res?;
            Ok(0)
        }
        Err(e) => Err(e),
    }
}

fn semctl_global_info(
    cmd: i32,
    semid: i32,
    arg: usize,
    cred: &Ucred,
    vmar: &Arc<crate::vm::vmar::Vmar>,
) -> Option<Result<usize>> {
    if cmd == IPC_INFO || cmd == SEM_INFO {
        if arg == 0 {
            return Some(Err(Errno::EFAULT));
        }
        let info = SemInfo::default();
        if let Err(e) = UserPtr::new(arg).write_bytes(vmar, &info.to_bytes()) {
            return Some(Err(e));
        }
        return Some(Ok(SEMMNI));
    }

    if cmd == SEM_STAT || cmd == SEM_STAT_ANY {
        if arg == 0 {
            return Some(Err(Errno::EFAULT));
        }
        let reg = SEM_REGISTRY.lock();
        let idx = semid as usize;
        let Some((&id, set_lock)) = reg.iter().nth(idx) else {
            return Some(Err(Errno::EINVAL));
        };
        let set = set_lock.lock();

        if cmd == SEM_STAT {
            if let Err(e) = check_read_permission(&set.ds.sem_perm, cred) {
                return Some(Err(e));
            }
        }

        if let Err(e) = UserPtr::new(arg).write_bytes(vmar, &set.ds.to_bytes()) {
            return Some(Err(e));
        }
        return Some(Ok(id as usize));
    }

    None
}

fn semctl_stat(
    set_lock: &SpinLock<SemSet>,
    arg: usize,
    cred: &Ucred,
    vmar: &Arc<crate::vm::vmar::Vmar>,
) -> Result<usize> {
    if arg == 0 {
        return Err(Errno::EFAULT);
    }
    let set = set_lock.lock();
    if set.destroyed {
        return Err(Errno::EIDRM);
    }
    check_read_permission(&set.ds.sem_perm, cred)?;
    UserPtr::new(arg).write_bytes(vmar, &set.ds.to_bytes())?;
    Ok(0)
}

fn semctl_set(
    set_lock: &SpinLock<SemSet>,
    arg: usize,
    cred: &Ucred,
    vmar: &Arc<crate::vm::vmar::Vmar>,
) -> Result<usize> {
    if arg == 0 {
        return Err(Errno::EFAULT);
    }
    let mut raw = [0u8; 104];
    UserPtr::new(arg).read_bytes(vmar, &mut raw)?;
    let new_ds = SemIdDs::from_bytes(&raw).ok_or(Errno::EINVAL)?;

    let mut set = set_lock.lock();
    if set.destroyed {
        return Err(Errno::EIDRM);
    }
    check_admin_permission(&set.ds.sem_perm, cred)?;

    set.ds.sem_perm.uid = new_ds.sem_perm.uid;
    set.ds.sem_perm.gid = new_ds.sem_perm.gid;
    set.ds.sem_perm.mode = (set.ds.sem_perm.mode & !0o777) | (new_ds.sem_perm.mode & 0o777);
    set.ds.sem_ctime = current_time_secs();
    Ok(0)
}

fn semctl_rmid(semid: i32, set_lock: &SpinLock<SemSet>, cred: &Ucred) -> Result<usize> {
    let mut reg = SEM_REGISTRY.lock();
    let mut set = set_lock.lock();
    if set.destroyed {
        return Err(Errno::EIDRM);
    }
    check_admin_permission(&set.ds.sem_perm, cred)?;

    set.destroyed = true;
    reg.remove(&semid);
    SEM_REGISTRY.unbind_key(set.key);
    clear_undo_for_sem(semid, None);
    set.wq.wake_all();
    Ok(0)
}

fn semctl_get_val(
    set_lock: &SpinLock<SemSet>,
    semnum: i32,
    cmd: i32,
    cred: &Ucred,
) -> Result<usize> {
    let set = set_lock.lock();
    if set.destroyed {
        return Err(Errno::EIDRM);
    }
    check_read_permission(&set.ds.sem_perm, cred)?;
    if semnum < 0 || (semnum as usize) >= set.nsems {
        return Err(Errno::EINVAL);
    }
    let sem = &set.sems[semnum as usize];
    match cmd {
        GETVAL => Ok(sem.val as usize),
        GETPID => Ok(sem.pid as usize),
        GETNCNT => Ok(sem.ncnt as usize),
        GETZCNT => Ok(sem.zcnt as usize),
        _ => Err(Errno::EINVAL),
    }
}

fn semctl_set_val(
    semid: i32,
    set_lock: &SpinLock<SemSet>,
    semnum: i32,
    arg: usize,
    cred: &Ucred,
    proc: &Arc<Proc>,
) -> Result<usize> {
    let val = arg as i32;
    if val < 0 || val > (SEMVMX as i32) {
        return Err(Errno::ERANGE);
    }

    let mut set = set_lock.lock();
    if set.destroyed {
        return Err(Errno::EIDRM);
    }
    check_write_permission(&set.ds.sem_perm, cred)?;
    if semnum < 0 || (semnum as usize) >= set.nsems {
        return Err(Errno::EINVAL);
    }

    set.sems[semnum as usize].val = val as u16;
    set.sems[semnum as usize].pid = proc.pid.as_u32();
    set.ds.sem_ctime = current_time_secs();
    clear_undo_for_sem(semid, Some(semnum as u16));
    set.wq.wake_all();
    Ok(0)
}

fn semctl_getall(
    set_lock: &SpinLock<SemSet>,
    arg: usize,
    cred: &Ucred,
    vmar: &Arc<crate::vm::vmar::Vmar>,
) -> Result<usize> {
    if arg == 0 {
        return Err(Errno::EFAULT);
    }
    let set = set_lock.lock();
    if set.destroyed {
        return Err(Errno::EIDRM);
    }
    check_read_permission(&set.ds.sem_perm, cred)?;

    let mut raw = Vec::with_capacity(set.nsems * 2);
    for sem in &set.sems {
        raw.extend_from_slice(&sem.val.to_ne_bytes());
    }
    UserPtr::new(arg).write_bytes(vmar, &raw)?;
    Ok(0)
}

fn semctl_setall(
    semid: i32,
    set_lock: &SpinLock<SemSet>,
    arg: usize,
    cred: &Ucred,
    proc: &Arc<Proc>,
    vmar: &Arc<crate::vm::vmar::Vmar>,
) -> Result<usize> {
    if arg == 0 {
        return Err(Errno::EFAULT);
    }
    let mut set = set_lock.lock();
    if set.destroyed {
        return Err(Errno::EIDRM);
    }
    check_write_permission(&set.ds.sem_perm, cred)?;

    let nsems = set.nsems;
    let mut raw = Vec::new();
    raw.resize(nsems * 2, 0u8);
    UserPtr::new(arg).read_bytes(vmar, &mut raw)?;

    let mut values = Vec::with_capacity(nsems);
    for i in 0..nsems {
        let val = u16::from_ne_bytes(raw[i * 2..i * 2 + 2].try_into().unwrap());
        if val > SEMVMX {
            return Err(Errno::ERANGE);
        }
        values.push(val);
    }

    for (i, val) in values.into_iter().enumerate() {
        set.sems[i].val = val;
        set.sems[i].pid = proc.pid.as_u32();
    }
    set.ds.sem_ctime = current_time_secs();
    clear_undo_for_sem(semid, None);
    set.wq.wake_all();
    Ok(0)
}

/// `semctl(2)` - Controls semaphore parameters, values, and lifecycle.
pub fn sem_ctl(
    semid: i32,
    semnum: i32,
    cmd: i32,
    arg: usize,
    cred: &Ucred,
    proc: &Arc<Proc>,
) -> Result<usize> {
    let vmar = proc.vmspace();

    if let Some(res) = semctl_global_info(cmd, semid, arg, cred, &vmar) {
        return res;
    }

    let set_lock = lookup_set(semid).ok_or(Errno::EINVAL)?;

    match cmd {
        IPC_STAT => semctl_stat(&set_lock, arg, cred, &vmar),
        IPC_SET => semctl_set(&set_lock, arg, cred, &vmar),
        IPC_RMID => semctl_rmid(semid, &set_lock, cred),
        GETVAL | GETPID | GETNCNT | GETZCNT => semctl_get_val(&set_lock, semnum, cmd, cred),
        SETVAL => semctl_set_val(semid, &set_lock, semnum, arg, cred, proc),
        GETALL => semctl_getall(&set_lock, arg, cred, &vmar),
        SETALL => semctl_setall(semid, &set_lock, arg, cred, proc, &vmar),
        _ => Err(Errno::EINVAL),
    }
}

/// Applies semaphore undo adjustments when a process terminates.
pub fn on_proc_exit(proc: &Proc) {
    undo::apply_proc_undo(proc, current_time_secs(), lookup_set);
}
