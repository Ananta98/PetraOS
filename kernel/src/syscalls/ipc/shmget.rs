//! sys_shmget system call handler.

use super::*;
use crate::ipc::shm::SHM_MANAGER;
use crate::syscalls::{wrap_syscall, SyscallResult};

#[wrap_syscall]
pub fn sys_shmget(key: i32, size: usize, shmflg: i32) -> SyscallResult {
    let (uid, gid) = current_uid_gid();
    let pid = current_pid_u32();

    let mut mgr = SHM_MANAGER.lock();
    let shmid = mgr.shmget(key, size, shmflg, uid, gid, pid)?;
    Ok(shmid as usize)
}
