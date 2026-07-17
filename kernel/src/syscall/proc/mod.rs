pub mod credentials;
pub mod execve;
pub mod exit;
pub mod fork;
pub mod pid;
pub mod wait4;
pub mod waitid;

pub use credentials::{
    syscall_getegid, syscall_geteuid, syscall_getgid, syscall_getresgid, syscall_getresuid,
    syscall_getuid, syscall_setfsgid, syscall_setfsuid, syscall_setgid, syscall_setregid,
    syscall_setresgid, syscall_setresuid, syscall_setreuid, syscall_setuid,
};
pub use execve::syscall_execve;
pub use exit::syscall_exit;
pub use fork::syscall_fork;
pub use pid::{
    syscall_getpgid, syscall_getpid, syscall_getppid, syscall_getsid, syscall_setpgid,
    syscall_setsid,
};
pub use wait4::syscall_wait4;
pub use waitid::syscall_waitid;

#[cfg(ktest)]
mod tests {
    use super::*;
    use crate::proc::pid_table::{PROCESS_TABLE, Pid};
    use crate::proc::process::Process;
    use crate::syscall::SyscallResult;
    use crate::vm::vma::VmaManager;
    use ostd::Error;
    use ostd::arch::cpu::context::UserContext;
    use ostd::prelude::ktest;

    #[ktest]
    fn test_new_proc_syscalls() {
        use alloc::sync::Arc;
        use ostd::mm::PageFlags;

        // Ensure init process exists.
        if PROCESS_TABLE.get_process(Pid::from_raw(1)).is_none() {
            let vm = Arc::new(VmaManager::new());
            let _init = Process::new(vm, "init");
        }

        let vm = Arc::new(VmaManager::new());
        vm.activate();
        let mut context = UserContext::default();

        // Map memory for pointer system calls.
        vm.map_region(0x10000, 4096, PageFlags::RW).unwrap();

        // Test getpid & getppid
        let getpid_res = syscall_getpid(0, 0, 0, 0, 0, 0, &vm, &mut context);
        match getpid_res {
            SyscallResult::Continue(pid) => {
                assert!(pid > 0);
            }
            _ => panic!("Expected SyscallResult::Continue"),
        }

        let getppid_res = syscall_getppid(0, 0, 0, 0, 0, 0, &vm, &mut context);
        match getppid_res {
            SyscallResult::Continue(_) => {}
            _ => panic!("Expected SyscallResult::Continue"),
        }

        // Test credentials getters
        let getuid_res = syscall_getuid(0, 0, 0, 0, 0, 0, &vm, &mut context);
        match getuid_res {
            SyscallResult::Continue(uid) => assert_eq!(uid, 0),
            _ => panic!("Expected SyscallResult::Continue"),
        }
        let getgid_res = syscall_getgid(0, 0, 0, 0, 0, 0, &vm, &mut context);
        match getgid_res {
            SyscallResult::Continue(gid) => assert_eq!(gid, 0),
            _ => panic!("Expected SyscallResult::Continue"),
        }

        // Test getresuid & getresgid
        // Pointers: ruid_ptr = 0x10000, euid_ptr = 0x10004, suid_ptr = 0x10008
        let getresuid_res =
            syscall_getresuid(0x10000, 0x10004, 0x10008, 0, 0, 0, &vm, &mut context);
        match getresuid_res {
            SyscallResult::Continue(0) => {
                // Read from mapped memory
                let mut buf = [0u8; 4];
                vm.copy_from_user(0x10000, &mut buf).unwrap();
                let ruid = u32::from_ne_bytes(buf);
                assert_eq!(ruid, 0);
            }
            _ => panic!("Expected SyscallResult::Continue(0)"),
        }

        // Test setuid / setgid
        let setuid_res = syscall_setuid(1000, 0, 0, 0, 0, 0, &vm, &mut context);
        match setuid_res {
            SyscallResult::Continue(0) => {
                // Verify euid is updated
                let geteuid_res = syscall_geteuid(0, 0, 0, 0, 0, 0, &vm, &mut context);
                match geteuid_res {
                    SyscallResult::Continue(euid) => assert_eq!(euid, 1000),
                    _ => panic!("Expected SyscallResult::Continue"),
                }
            }
            _ => panic!("Expected SyscallResult::Continue(0)"),
        }

        // Clean up: set credentials back to 0 so we can modify things as privileged
        // Reset credentials back to 0 directly so subsequent tests/assertions aren't affected
        PROCESS_TABLE.update_process(Process::current().pid, |p| {
            p.uid = 0;
            p.euid = 0;
            p.suid = 0;
            p.fsuid = 0;
        });

        // Test setpgid and getpgid
        let setpgid_res = syscall_setpgid(0, 0, 0, 0, 0, 0, &vm, &mut context);
        assert!(matches!(setpgid_res, SyscallResult::Continue(0)));

        let getpgid_res = syscall_getpgid(0, 0, 0, 0, 0, 0, &vm, &mut context);
        match getpgid_res {
            SyscallResult::Continue(pgid) => assert!(pgid > 0),
            _ => panic!("Expected SyscallResult::Continue"),
        }

        // Test getsid and setsid
        let getsid_res = syscall_getsid(0, 0, 0, 0, 0, 0, &vm, &mut context);
        match getsid_res {
            SyscallResult::Continue(sid) => assert!(sid > 0),
            _ => panic!("Expected SyscallResult::Continue"),
        }

        // Test setfsuid
        let setfsuid_res = syscall_setfsuid(2000, 0, 0, 0, 0, 0, &vm, &mut context);
        match setfsuid_res {
            SyscallResult::Continue(prev) => assert_eq!(prev, 0),
            _ => panic!("Expected SyscallResult::Continue"),
        }

        // Test waitid on a non-existent child / no children: should fail or return error
        // options = 4 (WEXITED), WNOHANG = 1.
        let waitid_res = syscall_waitid(0, 0, 0x10000, 5, 0, 0, &vm, &mut context);
        match waitid_res {
            SyscallResult::Continue(code) => {
                // Since init process (the fallback for Process::current()) has no children,
                // it should return negated ECHILD (InvalidArgs).
                assert_eq!(code as isize, -(Error::InvalidArgs as isize));
            }
            _ => panic!("Expected SyscallResult::Continue"),
        }

        // Unmap memory
        vm.unmap_region(0x10000, 4096).unwrap();
    }
}
