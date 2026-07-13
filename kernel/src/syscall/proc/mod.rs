pub(crate) mod execve;
pub(crate) mod exit;
pub(crate) mod fork;
pub(crate) mod wait4;

pub(crate) use execve::syscall_execve;
pub(crate) use exit::syscall_exit;
pub(crate) use fork::syscall_fork;
pub(crate) use wait4::syscall_wait4;

#[cfg(ktest)]
mod tests {
    use super::*;
    use crate::proc::pid_table::{PROCESS_TABLE, Pid};
    use crate::proc::process::Process;
    use crate::syscall::SyscallResult;
    use crate::vm::vma::VmaManager;
    use ostd::prelude::ktest;
    use ostd::arch::cpu::context::UserContext;

    #[ktest]
    fn test_proc_syscalls_basic() {
        use alloc::sync::Arc;

        // Ensure init process exists so Process::current() works.
        if PROCESS_TABLE.get_process(Pid::from_raw(1)).is_none() {
            let vm = Arc::new(VmaManager::new());
            let _init = Process::new(vm, "init");
        }

        let vm = Arc::new(VmaManager::new());
        let mut context = UserContext::default();

        // 1. Test syscall_fork
        let fork_res = syscall_fork(0, 0, 0, 0, 0, 0, &vm, &mut context);
        let child_pid = match fork_res {
            SyscallResult::Continue(val) => {
                // Should return child's PID (which is positive)
                assert!(val > 0);
                val as i32
            }
            _ => panic!("Expected SyscallResult::Continue"),
        };

        // 2. Test syscall_wait4 on the child (should return 0 with WNOHANG as child is still running)
        // options = 1 (WNOHANG)
        let wait_res = syscall_wait4(child_pid as usize, 0, 1, 0, 0, 0, &vm, &mut context);
        match wait_res {
            SyscallResult::Continue(val) => {
                assert_eq!(val as i32, 0);
            }
            _ => panic!("Expected SyscallResult::Continue"),
        }

        // 3. Test syscall_exit
        let exit_res = syscall_exit(42, 0, 0, 0, 0, 0, &vm, &mut context);
        match exit_res {
            SyscallResult::Exit(code) => {
                assert_eq!(code, 42);
            }
            _ => panic!("Expected SyscallResult::Exit"),
        }
    }
}
