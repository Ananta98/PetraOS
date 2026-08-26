//! sys_semget system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult, UserPtr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::arch::timer::hpet;
use crate::ipc::semaphore::SemaphoreManager;
use crate::ipc::semaphore::{
    GETALL, GETNCNT, GETPID, GETVAL, GETZCNT, IPC_RMID, IPC_SET, IPC_STAT,
    SEMAPHORE_MANAGER, SETALL, SETVAL, SemBuf, SemError, SemidDs, SemopResult,
};
use crate::ipc::shm::{SHM_MANAGER, ShmError, ShmidDs, ShmInfo};
use crate::proc::thread::ThreadState;


pub fn sys_semget(frame: &mut SyscallFrame) -> SyscallResult {
    let key = frame.arg1() as i32;
    let nsems = frame.arg2() as i32;
    let semflg = frame.arg3() as i32;

    let (uid, gid) = current_uid_gid();

    let mut mgr = SEMAPHORE_MANAGER.lock();
    let semid = mgr.semget(key, nsems, semflg, uid, gid)?;
    Ok(semid as usize)
}
