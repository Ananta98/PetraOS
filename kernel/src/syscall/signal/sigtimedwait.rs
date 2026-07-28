use crate::ipc::SigSet;
use crate::proc::process::Process;
use crate::syscall::time::{monotonic_ns, read_timespec};
use crate::syscall::{SyscallResult, to_continue_i32};
use crate::vm::vma::VmaManager;
use ostd::Error;
use ostd::arch::cpu::context::UserContext;

/// System call entry: `rt_sigtimedwait(uthese, uinfo, uts, sigsetsize)`.
pub fn syscall_rt_sigtimedwait(
    arg0: usize, // const sigset_t __user *uthese
    arg1: usize, // siginfo_t __user *uinfo
    arg2: usize, // const struct timespec __user *uts
    arg3: usize, // size_t sigsetsize
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let set_ptr = arg0;
    let info_ptr = arg1;
    let timeout_ptr = arg2;
    let sigsetsize = arg3;

    if sigsetsize != 8 || set_ptr == 0 {
        return to_continue_i32(Err(Error::InvalidArgs));
    }

    let mut raw_set = [0u8; 8];
    if vm.copy_from_user(set_ptr, &mut raw_set).is_err() {
        return to_continue_i32(Err(Error::InvalidArgs));
    }
    let target_mask = SigSet::from_u64(u64::from_le_bytes(raw_set));

    let deadline_ns = if timeout_ptr != 0 {
        let ts = match read_timespec(vm, timeout_ptr) {
            Ok(t) => t,
            Err(e) => return to_continue_i32(Err(e)),
        };
        if ts.tv_sec < 0 || ts.tv_nsec < 0 || ts.tv_nsec >= 1_000_000_000 {
            return to_continue_i32(Err(Error::InvalidArgs));
        }
        let duration_ns = ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64;
        Some(monotonic_ns().saturating_add(duration_ns))
    } else {
        None
    };

    let process = Process::current();
    let signals = process.signals.clone();

    loop {
        if let Some(info) = signals.queue.dequeue() {
            if target_mask.contains(info.signum) {
                if info_ptr != 0 {
                    let mut siginfo_buf = [0u8; 128];
                    siginfo_buf[0..4].copy_from_slice(&(info.signum as i32).to_ne_bytes());
                    let _ = vm.copy_to_user(info_ptr, &siginfo_buf);
                }
                return to_continue_i32(Ok(info.signum as i32));
            }
            // If dequeued a signal not in target_mask, re-enqueue info
            signals.queue.enqueue(info);
        }

        if let Some(dl) = deadline_ns {
            if monotonic_ns() >= dl {
                return to_continue_i32(Err(Error::IoError)); // EAGAIN / timeout
            }
        }
        core::hint::spin_loop();
    }
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::arch::cpu::context::UserContext;
    use ostd::prelude::ktest;

    #[ktest]
    fn test_rt_sigtimedwait_invalid_args() {
        let vm = VmaManager::new();
        let mut context = UserContext::default();

        // Invalid sigsetsize != 8
        let res = syscall_rt_sigtimedwait(0x1000, 0, 0, 4, 0, 0, &vm, &mut context);
        assert!(matches!(res, SyscallResult::Continue(val) if val == (-(Error::InvalidArgs as isize) as usize)));

        // Null set pointer
        let res = syscall_rt_sigtimedwait(0, 0, 0, 8, 0, 0, &vm, &mut context);
        assert!(matches!(res, SyscallResult::Continue(val) if val == (-(Error::InvalidArgs as isize) as usize)));
    }
}
