//! sys_shmget system call handler.

use super::*;
use crate::syscalls::SyscallResult;
use crate::arch::syscall::syscall::SyscallFrame;
use crate::ipc::shm::SHM_MANAGER;


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
