use crate::proc::pid_table::PROCESS_TABLE;
use crate::proc::process::Process;
use crate::syscall::SyscallResult;
use crate::vm::vma::VmaManager;
use ostd::arch::cpu::context::UserContext;

/// System call entry: terminate the calling process.
pub fn syscall_exit(
    arg0: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let code = arg0 as i32;
    let current_process = Process::current();
    PROCESS_TABLE.update_process(current_process.pid, |p| {
        p.exit(code);
    });
    SyscallResult::Exit(code)
}
