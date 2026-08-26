//! sys_shmget system call handler.

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


pub fn sys_shmget(frame: &mut SyscallFrame) -> SyscallResult {
    let key = frame.arg1() as i32;
    let size = frame.arg2() as usize;
    let shmflg = frame.arg3() as i32;

    let (uid, gid) = current_uid_gid();
    let pid = current_pid_u32();

    let mut mgr = SHM_MANAGER.lock();
    let shmid = mgr.shmget(key, size, shmflg, uid, gid, pid)?;
    Ok(shmid as usize)
}
