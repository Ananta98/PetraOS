#![allow(clippy::too_many_arguments)]

use crate::ipc::shm::{shm_at, shm_ctl, shm_dt, shm_get};
use crate::syscall::SyscallResult;
use crate::vm::vma::VmaManager;
use ostd::arch::cpu::context::UserContext;

pub fn syscall_shmget(
    arg0: usize,
    arg1: usize,
    arg2: usize,
    _arg3: usize,
    _arg4: usize,
    _arg5: usize,
    _vm: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let key = arg0;
    let size = arg1;
    let flags = arg2 as u32;

    match shm_get(key, size, flags) {
        Ok(shmid) => SyscallResult::Continue(shmid as usize),
        Err(e) => SyscallResult::Continue(-(e as isize) as usize),
    }
}

pub fn syscall_shmat(
    arg0: usize,
    arg1: usize,
    arg2: usize,
    _arg3: usize,
    _arg4: usize,
    _arg5: usize,
    _vm: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let shmid = arg0 as u32;
    let shmaddr = arg1;
    let flags = arg2 as u32;

    match shm_at(shmid, shmaddr, flags) {
        Ok(addr) => SyscallResult::Continue(addr),
        Err(e) => SyscallResult::Continue(-(e as isize) as usize),
    }
}

pub fn syscall_shmdt(
    arg0: usize,
    _arg1: usize,
    _arg2: usize,
    _arg3: usize,
    _arg4: usize,
    _arg5: usize,
    _vm: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let shmaddr = arg0;

    match shm_dt(shmaddr) {
        Ok(()) => SyscallResult::Continue(0),
        Err(e) => SyscallResult::Continue(-(e as isize) as usize),
    }
}

pub fn syscall_shmctl(
    arg0: usize,
    arg1: usize,
    _arg2: usize,
    _arg3: usize,
    _arg4: usize,
    _arg5: usize,
    _vm: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let shmid = arg0 as u32;
    let cmd = arg1 as u32;

    match shm_ctl(shmid, cmd) {
        Ok(()) => SyscallResult::Continue(0),
        Err(e) => SyscallResult::Continue(-(e as isize) as usize),
    }
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use crate::proc::pid_table::{PROCESS_TABLE, Pid};
    use crate::proc::process::Process;
    use crate::vm::vma::VmaManager;
    use alloc::sync::Arc;
    use ostd::mm::PAGE_SIZE;
    use ostd::prelude::ktest;

    #[ktest]
    fn test_shm_syscalls() {
        // Ensure init process exists.
        if PROCESS_TABLE.get_process(Pid::from_raw(1)).is_none() {
            let vm = Arc::new(VmaManager::new());
            let _init = Process::new(vm, "init");
        }

        let vm = Arc::new(VmaManager::new());
        let test_proc = Process::new(vm, "shm_syscall_proc");

        let thread = test_proc
            .spawn_thread("shm_syscall_thread", move || {
                let current_process = Process::current();
                current_process.vm.activate();
                let mut context = UserContext::default();

                let key = 0x99999;
                let size = PAGE_SIZE;
                // IPC_CREAT is 0o1000 = 512
                let flags = 512;

                // 1. SYS_shmget
                let get_res =
                    syscall_shmget(key, size, flags, 0, 0, 0, &current_process.vm, &mut context);
                let shmid = match get_res {
                    SyscallResult::Continue(id) => {
                        assert!(id > 0);
                        id
                    }
                    _ => panic!("Expected Continue"),
                };

                // 2. SYS_shmat
                let at_res = syscall_shmat(shmid, 0, 0, 0, 0, 0, &current_process.vm, &mut context);
                let addr = match at_res {
                    SyscallResult::Continue(addr) => {
                        assert!(addr > 0);
                        addr
                    }
                    _ => panic!("Expected Continue"),
                };

                // Write some data to verify
                let test_data = b"Syscall SHM test!";
                current_process.vm.copy_to_user(addr, test_data).unwrap();

                let mut buf = [0u8; 17];
                current_process.vm.copy_from_user(addr, &mut buf).unwrap();
                assert_eq!(&buf, test_data);

                // 3. SYS_shmdt
                let dt_res = syscall_shmdt(addr, 0, 0, 0, 0, 0, &current_process.vm, &mut context);
                match dt_res {
                    SyscallResult::Continue(0) => {}
                    _ => panic!("Expected Continue(0)"),
                }

                // 4. SYS_shmctl (IPC_RMID is 0)
                let ctl_res =
                    syscall_shmctl(shmid, 0, 0, 0, 0, 0, &current_process.vm, &mut context);
                match ctl_res {
                    SyscallResult::Continue(0) => {}
                    _ => panic!("Expected Continue(0)"),
                }
            })
            .unwrap();

        test_proc.join_thread(thread.tid);
    }
}
