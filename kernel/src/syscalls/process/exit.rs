use crate::arch::{ArchImpl, CpuArch};
use crate::proc::process::PROCESS_MANAGER;
use crate::proc::thread::{THREAD_MANAGER, exit_current_thread};

pub fn sys_exit(exit_code: i32) -> ! {
    let cpu_id = ArchImpl::cpu_id();
    let current_tid = THREAD_MANAGER.lock().current_thread_id(cpu_id).expect("No current thread");
    let current_pid = {
        let tm = THREAD_MANAGER.lock();
        tm.threads.get(&current_tid).map(|t| t.process_id).expect("No current process")
    };
    let _ = PROCESS_MANAGER.lock().exit_process(current_pid, exit_code);
    exit_current_thread(exit_code);
}