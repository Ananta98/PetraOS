// SPDX-License-Identifier: GPL-2.0

//! System V Semaphore system calls.

use core::time::Duration;
use ostd::arch::cpu::context::UserContext;

use crate::{
    proc::Thread,
    syscall::time::Timespec,
    uapi::errno::{Errno, Result},
    utils::UserPtr,
};

/// `semget(2)` - Allocates or accesses a System V semaphore set.
pub fn sys_semget(key: i32, nsems: i32, semflg: i32, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let cred = proc.cred();
    let semid = crate::ipc::sem::sem_get(key, nsems, semflg, &cred)?;
    Ok(semid as usize)
}

/// `semop(2)` - Performs atomic semaphore operations on a set.
pub fn sys_semop(semid: i32, sops: usize, nsops: usize, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let cred = proc.cred();
    crate::ipc::sem::sem_op(semid, sops, nsops, None, &cred, &proc)
}

/// `semctl(2)` - System V semaphore control operations.
pub fn sys_semctl(
    semid: i32,
    semnum: i32,
    cmd: i32,
    arg: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let cred = proc.cred();
    crate::ipc::sem::sem_ctl(semid, semnum, cmd, arg, &cred, &proc)
}

/// `semtimedop(2)` - Performs atomic semaphore operations with a timeout.
pub fn sys_semtimedop(
    semid: i32,
    sops: usize,
    nsops: usize,
    timeout_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let cred = proc.cred();

    let timeout = if timeout_ptr != 0 {
        let vmar = proc.vmspace();
        let mut raw = [0u8; 16];
        UserPtr::new(timeout_ptr).read_bytes(&vmar, &mut raw)?;
        let ts = Timespec::from_bytes(&raw).ok_or(Errno::EINVAL)?;
        if ts.tv_sec < 0 || ts.tv_nsec < 0 || ts.tv_nsec >= 1_000_000_000 {
            return Err(Errno::EINVAL);
        }
        Some(Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32))
    } else {
        None
    };

    crate::ipc::sem::sem_op(semid, sops, nsops, timeout, &cred, &proc)
}
