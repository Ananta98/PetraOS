use crate::proc::pid_table::Pid;
use crate::proc::thread::KernelThread;
use crate::proc::tid_table::THREAD_TABLE;
use crate::scheduler::nice::NiceWeight;
use crate::scheduler::{SchedClass, task_data::TaskData};
use crate::syscall::SyscallResult;
use crate::vm::vma::VmaManager;
use alloc::vec;
use ostd::Error;

const SCHED_NORMAL: usize = 0;
const SCHED_FIFO: usize = 1;
const SCHED_RR: usize = 2;

pub fn syscall_sched_getscheduler(
    arg0: usize,
    _arg1: usize,
    _arg2: usize,
    _arg3: usize,
    _arg4: usize,
    _arg5: usize,
    _vm: &VmaManager,
    _context: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    let pid = arg0 as i32;
    if pid < 0 {
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
        // Return the policy of the first thread in the process
        threads[0].clone()
    };

    let (class, _) = crate::scheduler::get_sched_data(&thread.task);

    let policy = match class {
        SchedClass::Fair { .. } | SchedClass::Idle => SCHED_NORMAL,
        SchedClass::RealTime { .. } => SCHED_RR,
    };

    SyscallResult::Continue(policy)
}

pub fn syscall_sched_setscheduler(
    arg0: usize,
    arg1: usize,
    arg2: usize,
    _arg3: usize,
    _arg4: usize,
    _arg5: usize,
    vm: &VmaManager,
    _context: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    let pid = arg0 as i32;
    let policy = arg1 as i32;
    let param_ptr = arg2;

    if pid < 0 {
        return SyscallResult::Continue(-(Error::InvalidArgs as isize) as usize);
    }

    if policy != SCHED_NORMAL as i32 && policy != SCHED_FIFO as i32 && policy != SCHED_RR as i32 {
        return SyscallResult::Continue(-(Error::InvalidArgs as isize) as usize);
    }

    let mut param_buf = [0u8; 4];
    if let Err(e) = vm.copy_from_user(param_ptr, &mut param_buf) {
        return SyscallResult::Continue(-(e as isize) as usize);
    }
    let priority = i32::from_ne_bytes(param_buf);

    let threads = if pid == 0 {
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

    let target_class = match policy as usize {
        SCHED_NORMAL => SchedClass::Fair {
            nice: NiceWeight::new(0),
        },
        SCHED_FIFO | SCHED_RR => SchedClass::RealTime {
            priority: priority.max(0) as u32,
        },
        _ => return SyscallResult::Continue(-(Error::InvalidArgs as isize) as usize),
    };

    for thread in threads {
        if let Some(data) = TaskData::from_task(&thread.task) {
            data.set_class(target_class);
        }
    }

    SyscallResult::Continue(0)
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::arch::cpu::context::UserContext;
    use ostd::prelude::ktest;

    #[ktest]
    fn test_sched_getscheduler_invalid_args() {
        let vm = VmaManager::new();
        let mut context = UserContext::default();

        let res = syscall_sched_getscheduler(-1_isize as usize, 0, 0, 0, 0, 0, &vm, &mut context);
        assert!(
            matches!(res, SyscallResult::Continue(val) if val == (-(Error::InvalidArgs as isize) as usize))
        );

        let res = syscall_sched_getscheduler(999999, 0, 0, 0, 0, 0, &vm, &mut context);
        assert!(matches!(res, SyscallResult::Continue(val) if val == ((-3_isize) as usize)));
    }

    #[ktest]
    fn test_sched_setscheduler_invalid_args() {
        let vm = VmaManager::new();
        let mut context = UserContext::default();

        let res =
            syscall_sched_setscheduler(-1_isize as usize, 0, 0x1000, 0, 0, 0, &vm, &mut context);
        assert!(
            matches!(res, SyscallResult::Continue(val) if val == (-(Error::InvalidArgs as isize) as usize))
        );

        let res = syscall_sched_setscheduler(0, 99, 0x1000, 0, 0, 0, &vm, &mut context);
        assert!(
            matches!(res, SyscallResult::Continue(val) if val == (-(Error::InvalidArgs as isize) as usize))
        );
    }
}
