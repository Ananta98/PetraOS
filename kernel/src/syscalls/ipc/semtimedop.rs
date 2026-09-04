//! sys_semtimedop system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult, UserPtr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::arch::timer::hpet;
use crate::ipc::semaphore::{
    SEMAPHORE_MANAGER, SemError,
};
use crate::proc::thread::ThreadState;


pub fn sys_semtimedop(frame: &mut SyscallFrame) -> SyscallResult {
    let semid = frame.arg1() as i32;
    let sops_ptr = frame.arg2();
    let nsops = frame.arg3() as usize;
    let timeout_ptr = frame.arg4();

    let ops = read_sembuf_slice(sops_ptr, nsops)?;
    let pid = current_pid_u32();

    // Parse optional timeout
    let deadline_ns: Option<u64> = if timeout_ptr != 0 {
        let ts_uptr = UserPtr::<TimeSpec>::from_u64(timeout_ptr);
        if !ts_uptr.is_valid() {
            return Err(SyscallError::EFAULT);
        }
        let ts = ts_uptr.read().ok_or(SyscallError::EFAULT)?;
        if ts.tv_sec < 0 || ts.tv_nsec < 0 || ts.tv_nsec >= 1_000_000_000 {
            return Err(SyscallError::EINVAL);
        }
        let dur_ns = (ts.tv_sec as u64)
            .saturating_mul(1_000_000_000)
            .saturating_add(ts.tv_nsec as u64);
        Some(hpet::elapsed_ns().saturating_add(dur_ns))
    } else {
        None
    };

    let thread_arc = crate::proc::current_thread().ok_or(SyscallError::ESRCH)?;

    loop {
        // Check deadline before attempting
        if let Some(dl) = deadline_ns {
            if hpet::elapsed_ns() >= dl {
                return Err(SyscallError::ETIMEDOUT);
            }
        }

        let result = {
            let mut mgr = SEMAPHORE_MANAGER.lock();
            mgr.semop_try(semid, &ops, thread_arc.clone(), pid, false)
        };

        match result {
            Ok(crate::ipc::semaphore::SemopResult::Done) => return Ok(0),
            Ok(crate::ipc::semaphore::SemopResult::Block { .. }) => {
                {
                    let mut t = thread_arc.lock();
                    t.state = ThreadState::Sleeping;
                }
                crate::sched::schedule(false);

                // Check timeout on wakeup
                if let Some(dl) = deadline_ns {
                    if crate::arch::timer::hpet::elapsed_ns() >= dl {
                        return Err(SyscallError::ETIMEDOUT);
                    }
                }

                let retry = {
                    let mut mgr = SEMAPHORE_MANAGER.lock();
                    mgr.semop_retry(semid, &ops, pid)
                };

                match retry {
                    Ok(true) => return Err(SyscallError::EIDRM),
                    Ok(false) => return Ok(0),
                    Err(SemError::WouldBlock) => continue,
                    Err(e) => return Err(e.into()),
                }
            }
            Err(e) => return Err(e.into()),
        }
    }
}
