use crate::arch::{ArchImpl, CpuArch};
use crate::proc::process_manager::PROCESS_MANAGER;
use crate::proc::thread_manager::THREAD_MANAGER;

pub fn sys_fork() -> u64 {
    let cpu_id = ArchImpl::cpu_id();
    let current_tid = THREAD_MANAGER.lock().current_thread_id(cpu_id).expect("No current thread");
    let current_pid = {
        let tm = THREAD_MANAGER.lock();
        tm.threads.get(&current_tid).map(|t| t.process_id).expect("No current process")
    };

    // Create process in PM
    let mut pm = PROCESS_MANAGER.lock();
    let child_pid = pm.create_process(Some(current_pid));

    log::info!("Syscall: fork() created child process with PID {:?}", child_pid);

    child_pid.0
}
