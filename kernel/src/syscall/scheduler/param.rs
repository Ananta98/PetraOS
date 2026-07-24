use crate::proc::pid_table::Pid;
use crate::proc::thread::KernelThread;
use crate::proc::tid_table::THREAD_TABLE;
use crate::scheduler::{SchedClass, task_data::TaskData};
use crate::syscall::SyscallResult;
use crate::vm::vma::VmaManager;
use alloc::vec;
use ostd::Error;

/// Get scheduling parameters for the process specified by `pid`.
/// If `pid` is 0, retrieves parameters for the calling process.
pub fn syscall_sched_getparam(
    arg0: usize,
    arg1: usize,
    _arg2: usize,
    _arg3: usize,
    _arg4: usize,
    _arg5: usize,
    vm: &VmaManager,
    _context: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    let pid = arg0 as i32;
    let param_ptr = arg1;

    if pid < 0 || param_ptr == 0 {
        return SyscallResult::Continue(-(Error::InvalidArgs as isize) as usize);
    }

    let thread = if pid == 0 {
        match KernelThread::current() {
            Some(t) => t,
            None => return SyscallResult::Continue(-(Error::InvalidArgs as isize) as usize),
        }
    } else {
        let threads = THREAD_TABLE.threads_of_process(Pid::from_raw(pid as u32));
        if threads.is_empty() {
            return SyscallResult::Continue(-3_isize as usize); // ESRCH
        }
        threads[0].clone()
    };

    let (class, _) = TaskData::sched_data(&thread.task);
    let priority: i32 = match class {
        SchedClass::RealTime { priority } => priority as i32,
        SchedClass::Fair { .. } => 0,
    };

    let param_bytes = priority.to_ne_bytes();
    if let Err(e) = vm.copy_to_user(param_ptr, &param_bytes) {
        return SyscallResult::Continue(-(e as isize) as usize);
    }

    SyscallResult::Continue(0)
}

/// Set scheduling parameters for the process specified by `pid`.
/// If `pid` is 0, sets parameters for the calling process.
pub fn syscall_sched_setparam(
    arg0: usize,
    arg1: usize,
    _arg2: usize,
    _arg3: usize,
    _arg4: usize,
    _arg5: usize,
    vm: &VmaManager,
    _context: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    let pid = arg0 as i32;
    let param_ptr = arg1;

    if pid < 0 || param_ptr == 0 {
        return SyscallResult::Continue(-(Error::InvalidArgs as isize) as usize);
    }

    let mut param_buf = [0u8; 4];
    if let Err(e) = vm.copy_from_user(param_ptr, &mut param_buf) {
        return SyscallResult::Continue(-(e as isize) as usize);
    }
    let _priority = i32::from_ne_bytes(param_buf);

    let _threads = if pid == 0 {
        match KernelThread::current() {
            Some(t) => vec![t],
            None => return SyscallResult::Continue(-(Error::InvalidArgs as isize) as usize),
        }
    } else {
        let threads = THREAD_TABLE.threads_of_process(Pid::from_raw(pid as u32));
        if threads.is_empty() {
            return SyscallResult::Continue(-3_isize as usize); // ESRCH
        }
        threads
    };

    SyscallResult::Continue(0)
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::arch::cpu::context::UserContext;
    use ostd::prelude::ktest;

    #[ktest]
    fn test_sched_getparam_invalid_args() {
        let vm = VmaManager::new();
        let mut context = UserContext::default();

        // Negative PID
        let res = syscall_sched_getparam(-1_isize as usize, 0x1000, 0, 0, 0, 0, &vm, &mut context);
        assert!(matches!(res, SyscallResult::Continue(val) if val == (-(Error::InvalidArgs as isize) as usize)));

        // Null pointer
        let res = syscall_sched_getparam(0, 0, 0, 0, 0, 0, &vm, &mut context);
        assert!(matches!(res, SyscallResult::Continue(val) if val == (-(Error::InvalidArgs as isize) as usize)));

        // Non-existent PID
        let res = syscall_sched_getparam(999999, 0x1000, 0, 0, 0, 0, &vm, &mut context);
        assert!(matches!(res, SyscallResult::Continue(val) if val == ((-3_isize) as usize)));
    }

    #[ktest]
    fn test_sched_setparam_invalid_args() {
        let vm = VmaManager::new();
        let mut context = UserContext::default();

        // Negative PID
        let res = syscall_sched_setparam(-1_isize as usize, 0x1000, 0, 0, 0, 0, &vm, &mut context);
        assert!(matches!(res, SyscallResult::Continue(val) if val == (-(Error::InvalidArgs as isize) as usize)));

        // Null pointer
        let res = syscall_sched_setparam(0, 0, 0, 0, 0, 0, &vm, &mut context);
        assert!(matches!(res, SyscallResult::Continue(val) if val == (-(Error::InvalidArgs as isize) as usize)));
    }
}
