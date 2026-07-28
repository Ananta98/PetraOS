use crate::proc::pid_table::Pid;
use crate::proc::thread::KernelThread;
use crate::proc::tid_table::THREAD_TABLE;
use crate::syscall::{SyscallResult, to_continue_i32};
use crate::vm::vma::VmaManager;
use ostd::Error;
use ostd::arch::cpu::context::UserContext;

/// `sched_getaffinity(pid, cpusetsize, mask)` — SYS_sched_getaffinity = 204
pub fn syscall_sched_getaffinity(
    arg0: usize,
    arg1: usize,
    arg2: usize,
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let pid = arg0 as i32;
    let cpusetsize = arg1;
    let mask_ptr = arg2;

    if pid < 0 || cpusetsize == 0 || mask_ptr == 0 {
        return to_continue_i32(Err(Error::InvalidArgs));
    }

    if pid != 0 {
        let threads = THREAD_TABLE.threads_of_process(Pid::from_raw(pid as u32));
        if threads.is_empty() {
            return SyscallResult::Continue(-3_isize as usize); // ESRCH
        }
    } else if KernelThread::current().is_none() {
        return to_continue_i32(Err(Error::InvalidArgs));
    }

    // Default CPU mask: enable all CPUs up to cpusetsize (e.g. 0xFF for first 8 CPUs)
    let copy_bytes = cpusetsize.min(128);
    let mut mask_buf = alloc::vec![0u8; copy_bytes];
    if !mask_buf.is_empty() {
        mask_buf[0] = 0xFF; // Bitmask for CPUs 0-7
    }

    to_continue_i32(vm.copy_to_user(mask_ptr, &mask_buf).map(|_| copy_bytes as i32))
}

/// `sched_setaffinity(pid, cpusetsize, mask)` — SYS_sched_setaffinity = 203
pub fn syscall_sched_setaffinity(
    arg0: usize,
    arg1: usize,
    arg2: usize,
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let pid = arg0 as i32;
    let cpusetsize = arg1;
    let mask_ptr = arg2;

    if pid < 0 || cpusetsize == 0 || mask_ptr == 0 {
        return to_continue_i32(Err(Error::InvalidArgs));
    }

    if pid != 0 {
        let threads = THREAD_TABLE.threads_of_process(Pid::from_raw(pid as u32));
        if threads.is_empty() {
            return SyscallResult::Continue(-3_isize as usize); // ESRCH
        }
    } else if KernelThread::current().is_none() {
        return to_continue_i32(Err(Error::InvalidArgs));
    }

    let mut mask_buf = alloc::vec![0u8; cpusetsize.min(128)];
    if vm.copy_from_user(mask_ptr, &mut mask_buf).is_err() {
        return to_continue_i32(Err(Error::InvalidArgs));
    }

    to_continue_i32(Ok(0))
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::arch::cpu::context::UserContext;
    use ostd::prelude::ktest;

    #[ktest]
    fn test_sched_getaffinity_invalid_args() {
        let vm = VmaManager::new();
        let mut context = UserContext::default();

        // Negative PID
        let res = syscall_sched_getaffinity(-1_isize as usize, 8, 0x1000, 0, 0, 0, &vm, &mut context);
        assert!(matches!(res, SyscallResult::Continue(val) if val == (-(Error::InvalidArgs as isize) as usize)));

        // Zero cpusetsize
        let res = syscall_sched_getaffinity(0, 0, 0x1000, 0, 0, 0, &vm, &mut context);
        assert!(matches!(res, SyscallResult::Continue(val) if val == (-(Error::InvalidArgs as isize) as usize)));

        // Null mask pointer
        let res = syscall_sched_getaffinity(0, 8, 0, 0, 0, 0, &vm, &mut context);
        assert!(matches!(res, SyscallResult::Continue(val) if val == (-(Error::InvalidArgs as isize) as usize)));

        // Non-existent PID
        let res = syscall_sched_getaffinity(999999, 8, 0x1000, 0, 0, 0, &vm, &mut context);
        assert!(matches!(res, SyscallResult::Continue(val) if val == ((-3_isize) as usize)));
    }

    #[ktest]
    fn test_sched_setaffinity_invalid_args() {
        let vm = VmaManager::new();
        let mut context = UserContext::default();

        // Negative PID
        let res = syscall_sched_setaffinity(-1_isize as usize, 8, 0x1000, 0, 0, 0, &vm, &mut context);
        assert!(matches!(res, SyscallResult::Continue(val) if val == (-(Error::InvalidArgs as isize) as usize)));

        // Null mask pointer
        let res = syscall_sched_setaffinity(0, 8, 0, 0, 0, 0, &vm, &mut context);
        assert!(matches!(res, SyscallResult::Continue(val) if val == (-(Error::InvalidArgs as isize) as usize)));
    }
}
