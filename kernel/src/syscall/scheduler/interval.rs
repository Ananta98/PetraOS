use crate::proc::pid_table::Pid;
use crate::proc::thread::KernelThread;
use crate::proc::tid_table::THREAD_TABLE;
use crate::syscall::{SyscallResult};
use crate::vm::vma::VmaManager;
use ostd::Error;
use ostd::arch::cpu::context::UserContext;

/// `sched_rr_get_interval(pid, tp)` — SYS_sched_rr_get_interval = 148
pub fn syscall_sched_rr_get_interval(
    arg0: usize,
    arg1: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let pid = arg0 as i32;
    let tp_ptr = arg1;

    if pid < 0 || tp_ptr == 0 {
        return SyscallResult::from_err(Error::InvalidArgs);
    }

    if pid != 0 {
        let threads = THREAD_TABLE.threads_of_process(Pid::from_raw(pid as u32));
        if threads.is_empty() {
            return SyscallResult::Return(-3_isize as usize); // ESRCH
        }
    } else if KernelThread::current().is_none() {
        return SyscallResult::from_err(Error::InvalidArgs);
    }

    // Default round-robin time quantum: 100 ms (0 sec, 100,000,000 ns)
    let tv_sec: i64 = 0;
    let tv_nsec: i64 = 100_000_000;

    let mut buf = [0u8; 16];
    buf[0..8].copy_from_slice(&tv_sec.to_ne_bytes());
    buf[8..16].copy_from_slice(&tv_nsec.to_ne_bytes());

    SyscallResult::from_result(vm.copy_to_user(tp_ptr, &buf).map(|_| 0))
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::arch::cpu::context::UserContext;
    use ostd::prelude::ktest;

    #[ktest]
    fn test_sched_rr_get_interval_invalid_args() {
        let vm = VmaManager::new();
        let mut context = UserContext::default();

        // Negative PID
        let res =
            syscall_sched_rr_get_interval(-1_isize as usize, 0x1000, 0, 0, 0, 0, &vm, &mut context);
        assert!(
            matches!(res, SyscallResult::Return(val) if val == (-(Error::InvalidArgs as isize) as usize))
        );

        // Null timespec pointer
        let res = syscall_sched_rr_get_interval(0, 0, 0, 0, 0, 0, &vm, &mut context);
        assert!(
            matches!(res, SyscallResult::Return(val) if val == (-(Error::InvalidArgs as isize) as usize))
        );

        // Non-existent PID
        let res = syscall_sched_rr_get_interval(999999, 0x1000, 0, 0, 0, 0, &vm, &mut context);
        assert!(matches!(res, SyscallResult::Return(val) if val == ((-3_isize) as usize)));
    }
}
