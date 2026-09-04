//! sys_semop system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::ipc::semaphore::{
    SEMAPHORE_MANAGER, SemError, SemopResult,
};
use crate::proc::thread::ThreadState;


pub fn sys_semop(frame: &mut SyscallFrame) -> SyscallResult {
    let semid = frame.arg1() as i32;
    let sops_ptr = frame.arg2();
    let nsops = frame.arg3() as usize;

    let ops = read_sembuf_slice(sops_ptr, nsops)?;
    let pid = current_pid_u32();

    let thread_arc = crate::proc::current_thread().ok_or(SyscallError::ESRCH)?;

    loop {
        let result = {
            let mut mgr = SEMAPHORE_MANAGER.lock();
            mgr.semop_try(semid, &ops, thread_arc.clone(), pid, false)
        };

        match result {
            Ok(SemopResult::Done) => return Ok(0),
            Ok(SemopResult::Block { .. }) => {
                // Put current thread to sleep
                {
                    let mut t = thread_arc.lock();
                    t.state = ThreadState::Sleeping;
                }
                crate::sched::schedule(false);

                // Woken up – retry or check for removal
                let retry = {
                    let mut mgr = SEMAPHORE_MANAGER.lock();
                    mgr.semop_retry(semid, &ops, pid)
                };

                match retry {
                    Ok(true) => return Err(SyscallError::EIDRM),
                    Ok(false) => return Ok(0),
                    Err(SemError::WouldBlock) => continue, // spurious wakeup – loop again
                    Err(e) => return Err(e.into()),
                }
            }
            Err(e) => return Err(e.into()),
        }
    }
}
