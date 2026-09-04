//! sys_shmdt system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::ipc::shm::SHM_MANAGER;


pub fn sys_shmdt(frame: &mut SyscallFrame) -> SyscallResult {
    let shmaddr = frame.arg1();
    let pid = current_pid_u32();

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();

    let addr_space_arc = alloc::sync::Arc::clone(&proc.address_space);
    let mut addr_space = addr_space_arc.lock();
    let mut mgr = SHM_MANAGER.lock();

    mgr.shmdt(shmaddr, pid, &mut addr_space)?;

    Ok(0)
}
