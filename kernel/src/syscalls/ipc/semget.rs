//! sys_semget system call handler.

use super::*;
use crate::ipc::semaphore::SEMAPHORE_MANAGER;
use crate::syscalls::{wrap_syscall, SyscallResult};

#[wrap_syscall]
pub fn sys_semget(key: i32, nsems: i32, semflg: i32) -> SyscallResult {
    let (uid, gid) = current_uid_gid();

    let mut mgr = SEMAPHORE_MANAGER.lock();
    let semid = mgr.semget(key, nsems, semflg, uid, gid)?;
    Ok(semid as usize)
}
