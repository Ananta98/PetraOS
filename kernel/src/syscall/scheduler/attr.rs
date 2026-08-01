use crate::proc::pid_table::Pid;
use crate::proc::thread::KernelThread;
use crate::proc::tid_table::THREAD_TABLE;
use crate::syscall::{SyscallResult};
use crate::vm::vma::VmaManager;
use ostd::Error;
use ostd::arch::cpu::context::UserContext;

#[repr(C)]
#[derive(Default, Copy, Clone)]
pub struct SchedAttr {
    pub size: u32,
    pub sched_policy: u32,
    pub sched_flags: u64,
    pub sched_nice: i32,
    pub sched_priority: u32,
    pub sched_runtime: u64,
    pub sched_deadline: u64,
    pub sched_period: u64,
    pub sched_util_min: u32,
    pub sched_util_max: u32,
}

/// `sched_getattr(pid, attr, size, flags)` — SYS_sched_getattr = 315
pub fn syscall_sched_getattr(
    arg0: usize,
    arg1: usize,
    arg2: usize,
    arg3: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let pid = arg0 as i32;
    let attr_ptr = arg1;
    let size = arg2 as u32;
    let flags = arg3 as u32;

    if pid < 0 || attr_ptr == 0 || size < 56 || flags != 0 {
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

    let attr = SchedAttr {
        size: 56,
        sched_policy: 0, // SCHED_OTHER / SCHED_NORMAL
        sched_flags: 0,
        sched_nice: 0,
        sched_priority: 0,
        sched_runtime: 0,
        sched_deadline: 0,
        sched_period: 0,
        sched_util_min: 0,
        sched_util_max: 1024,
    };

    let mut buf = [0u8; 56];
    buf[0..4].copy_from_slice(&attr.size.to_ne_bytes());
    buf[4..8].copy_from_slice(&attr.sched_policy.to_ne_bytes());
    buf[8..16].copy_from_slice(&attr.sched_flags.to_ne_bytes());
    buf[16..20].copy_from_slice(&attr.sched_nice.to_ne_bytes());
    buf[20..24].copy_from_slice(&attr.sched_priority.to_ne_bytes());
    buf[24..32].copy_from_slice(&attr.sched_runtime.to_ne_bytes());
    buf[32..40].copy_from_slice(&attr.sched_deadline.to_ne_bytes());
    buf[40..48].copy_from_slice(&attr.sched_period.to_ne_bytes());
    buf[48..52].copy_from_slice(&attr.sched_util_min.to_ne_bytes());
    buf[52..56].copy_from_slice(&attr.sched_util_max.to_ne_bytes());

    SyscallResult::from_result(vm.copy_to_user(attr_ptr, &buf).map(|_| 0))
}

/// `sched_setattr(pid, attr, flags)` — SYS_sched_setattr = 314
pub fn syscall_sched_setattr(
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
    let attr_ptr = arg1;
    let flags = arg2 as u32;

    if pid < 0 || attr_ptr == 0 || flags != 0 {
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

    let mut size_buf = [0u8; 4];
    if vm.copy_from_user(attr_ptr, &mut size_buf).is_err() {
        return SyscallResult::from_err(Error::InvalidArgs);
    }
    let size = u32::from_ne_bytes(size_buf);
    if size < 56 {
        return SyscallResult::from_err(Error::InvalidArgs);
    }

    SyscallResult::from_result(Ok(0))
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::arch::cpu::context::UserContext;
    use ostd::prelude::ktest;

    #[ktest]
    fn test_sched_getattr_invalid_args() {
        let vm = VmaManager::new();
        let mut context = UserContext::default();

        // Negative PID
        let res = syscall_sched_getattr(-1_isize as usize, 0x1000, 56, 0, 0, 0, &vm, &mut context);
        assert!(
            matches!(res, SyscallResult::Return(val) if val == (-(Error::InvalidArgs as isize) as usize))
        );

        // Small size (< 56)
        let res = syscall_sched_getattr(0, 0x1000, 32, 0, 0, 0, &vm, &mut context);
        assert!(
            matches!(res, SyscallResult::Return(val) if val == (-(Error::InvalidArgs as isize) as usize))
        );

        // Null attr pointer
        let res = syscall_sched_getattr(0, 0, 56, 0, 0, 0, &vm, &mut context);
        assert!(
            matches!(res, SyscallResult::Return(val) if val == (-(Error::InvalidArgs as isize) as usize))
        );

        // Non-existent PID
        let res = syscall_sched_getattr(999999, 0x1000, 56, 0, 0, 0, &vm, &mut context);
        assert!(matches!(res, SyscallResult::Return(val) if val == ((-3_isize) as usize)));
    }

    #[ktest]
    fn test_sched_setattr_invalid_args() {
        let vm = VmaManager::new();
        let mut context = UserContext::default();

        // Negative PID
        let res = syscall_sched_setattr(-1_isize as usize, 0x1000, 0, 0, 0, 0, &vm, &mut context);
        assert!(
            matches!(res, SyscallResult::Return(val) if val == (-(Error::InvalidArgs as isize) as usize))
        );

        // Null attr pointer
        let res = syscall_sched_setattr(0, 0, 0, 0, 0, 0, &vm, &mut context);
        assert!(
            matches!(res, SyscallResult::Return(val) if val == (-(Error::InvalidArgs as isize) as usize))
        );
    }
}
