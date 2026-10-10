// SPDX-License-Identifier: GPL-2.0

//! System V Shared Memory system calls.

use ostd::arch::cpu::context::UserContext;

use crate::{
    api::errno::{Errno, Result},
    proc::thread::Thread,
};

/// `shmget(2)` - Allocates a System V shared memory segment.
pub fn sys_shmget(key: i32, size: usize, shmflg: i32, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let cred = proc.cred();
    let shmid = crate::ipc::shm::shm_get(key, size, shmflg, &cred, proc.pid)?;
    Ok(shmid as usize)
}

/// `shmat(2)` - Attaches a System V shared memory segment to the calling address space.
pub fn sys_shmat(shmid: i32, shmaddr: usize, shmflg: i32, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let cred = proc.cred();
    crate::ipc::shm::shm_at(shmid, shmaddr, shmflg, &cred, &proc)
}

/// `shmctl(2)` - System V shared memory control operations.
pub fn sys_shmctl(shmid: i32, cmd: i32, buf: usize, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let cred = proc.cred();
    crate::ipc::shm::shm_ctl(shmid, cmd, buf, &cred, &proc)
}

/// `shmdt(2)` - Detaches a System V shared memory segment from the calling address space.
pub fn sys_shmdt(shmaddr: usize, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    crate::ipc::shm::shm_dt(shmaddr, &proc)?;
    Ok(0)
}
