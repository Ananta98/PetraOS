//! sys_semget system call handler.

use super::*;
use crate::syscalls::SyscallResult;
use crate::arch::syscall::syscall::SyscallFrame;
use crate::ipc::semaphore::SEMAPHORE_MANAGER;


pub fn sys_semget(frame: &mut SyscallFrame) -> SyscallResult {
    let key = frame.arg1() as i32;
    let nsems = frame.arg2() as i32;
    let semflg = frame.arg3() as i32;

    let (uid, gid) = current_uid_gid();

    let mut mgr = SEMAPHORE_MANAGER.lock();
    let semid = mgr.semget(key, nsems, semflg, uid, gid)?;
    Ok(semid as usize)
}
