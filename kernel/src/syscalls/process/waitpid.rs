use crate::arch::{ArchImpl, CpuArch};
use crate::proc::process::ProcessId;
use crate::proc::process_manager::PROCESS_MANAGER;
use crate::proc::thread_manager::THREAD_MANAGER;

pub fn sys_waitpid(pid: i64) -> u64 {
    let cpu_id = ArchImpl::cpu_id();
    let current_tid = THREAD_MANAGER.lock().current_thread_id(cpu_id).expect("No current thread");
    let current_pid = {
        let tm = THREAD_MANAGER.lock();
        tm.threads.get(&current_tid).map(|t| t.process_id).expect("No current process")
    };

    let target_pid = ProcessId(pid as u64);

    loop {
        let mut pm = PROCESS_MANAGER.lock();
        match pm.wait_pid(current_pid, target_pid) {
            Ok(exit_code) => return exit_code as u64,
            Err("Target process is not a zombie") => {
                // Drop the lock before yielding to avoid deadlock
                drop(pm);
                crate::proc::yield_now();
            }
            Err(_) => return u64::MAX, // E.g., target process not found or not child
        }
    }
}
