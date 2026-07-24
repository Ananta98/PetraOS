use crate::syscall::{SyscallResult, to_continue_i32};
use crate::vm::vma::VmaManager;
use ostd::Error;

const SCHED_NORMAL: i32 = 0;
const SCHED_FIFO: i32 = 1;
const SCHED_RR: i32 = 2;
const SCHED_BATCH: i32 = 3;
const SCHED_ISO: i32 = 4;
const SCHED_IDLE: i32 = 5;
const SCHED_DEADLINE: i32 = 6;

/// Returns the maximum priority value for the scheduling policy specified by `policy`.
pub fn syscall_sched_get_priority_max(
    arg0: usize,
    _arg1: usize,
    _arg2: usize,
    _arg3: usize,
    _arg4: usize,
    _arg5: usize,
    _vm: &VmaManager,
    _context: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    let policy = arg0 as i32;
    match policy {
        SCHED_FIFO | SCHED_RR => to_continue_i32(Ok(99)),
        SCHED_NORMAL | SCHED_BATCH | SCHED_ISO | SCHED_IDLE | SCHED_DEADLINE => {
            to_continue_i32(Ok(0))
        }
        _ => to_continue_i32(Err(Error::InvalidArgs)),
    }
}

/// Returns the minimum priority value for the scheduling policy specified by `policy`.
pub fn syscall_sched_get_priority_min(
    arg0: usize,
    _arg1: usize,
    _arg2: usize,
    _arg3: usize,
    _arg4: usize,
    _arg5: usize,
    _vm: &VmaManager,
    _context: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    let policy = arg0 as i32;
    match policy {
        SCHED_FIFO | SCHED_RR => to_continue_i32(Ok(1)),
        SCHED_NORMAL | SCHED_BATCH | SCHED_ISO | SCHED_IDLE | SCHED_DEADLINE => {
            to_continue_i32(Ok(0))
        }
        _ => to_continue_i32(Err(Error::InvalidArgs)),
    }
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::arch::cpu::context::UserContext;
    use ostd::prelude::ktest;

    #[ktest]
    fn test_sched_get_priority_max() {
        let vm = VmaManager::new();
        let mut context = UserContext::default();

        // Real-time policies should return 99
        let res = syscall_sched_get_priority_max(
            SCHED_FIFO as usize,
            0,
            0,
            0,
            0,
            0,
            &vm,
            &mut context,
        );
        assert!(matches!(res, SyscallResult::Continue(99)));

        let res =
            syscall_sched_get_priority_max(SCHED_RR as usize, 0, 0, 0, 0, 0, &vm, &mut context);
        assert!(matches!(res, SyscallResult::Continue(99)));

        // Non-real-time policies should return 0
        let res = syscall_sched_get_priority_max(
            SCHED_NORMAL as usize,
            0,
            0,
            0,
            0,
            0,
            &vm,
            &mut context,
        );
        assert!(matches!(res, SyscallResult::Continue(0)));

        let res = syscall_sched_get_priority_max(
            SCHED_BATCH as usize,
            0,
            0,
            0,
            0,
            0,
            &vm,
            &mut context,
        );
        assert!(matches!(res, SyscallResult::Continue(0)));

        // Invalid policy should return error (-EINVAL)
        let res = syscall_sched_get_priority_max(999, 0, 0, 0, 0, 0, &vm, &mut context);
        assert!(matches!(res, SyscallResult::Continue(val) if val == (-(Error::InvalidArgs as isize) as usize)));
    }

    #[ktest]
    fn test_sched_get_priority_min() {
        let vm = VmaManager::new();
        let mut context = UserContext::default();

        // Real-time policies should return 1
        let res = syscall_sched_get_priority_min(
            SCHED_FIFO as usize,
            0,
            0,
            0,
            0,
            0,
            &vm,
            &mut context,
        );
        assert!(matches!(res, SyscallResult::Continue(1)));

        let res =
            syscall_sched_get_priority_min(SCHED_RR as usize, 0, 0, 0, 0, 0, &vm, &mut context);
        assert!(matches!(res, SyscallResult::Continue(1)));

        // Non-real-time policies should return 0
        let res = syscall_sched_get_priority_min(
            SCHED_NORMAL as usize,
            0,
            0,
            0,
            0,
            0,
            &vm,
            &mut context,
        );
        assert!(matches!(res, SyscallResult::Continue(0)));

        // Invalid policy should return error (-EINVAL)
        let res = syscall_sched_get_priority_min(999, 0, 0, 0, 0, 0, &vm, &mut context);
        assert!(matches!(res, SyscallResult::Continue(val) if val == (-(Error::InvalidArgs as isize) as usize)));
    }
}
