//! sys_shmat system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::ipc::shm::SHM_MANAGER;


pub fn sys_shmat(frame: &mut SyscallFrame) -> SyscallResult {
    let shmid = frame.arg1() as i32;
    let shmaddr = frame.arg2();
    let shmflg = frame.arg3() as i32;

    let (uid, gid) = current_uid_gid();
    let pid = current_pid_u32();

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let mut proc = proc_arc.lock();

    let addr_space_arc = alloc::sync::Arc::clone(&proc.address_space);
    let mut addr_space = addr_space_arc.lock();
    let mut mgr = SHM_MANAGER.lock();

    let vaddr = mgr.shmat(
        shmid,
        shmaddr,
        shmflg,
        uid,
        gid,
        pid,
        &mut addr_space,
        &mut proc.mmap_bump,
    )?;

    Ok(vaddr as usize)
}
