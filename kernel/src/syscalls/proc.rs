use crate::arch::syscall::syscall::SyscallFrame;
use super::SyscallResult;

/// `sys_yield` (SYS_YIELD = 24)
/// Yield the CPU to another runnable thread.
pub fn sys_yield(_frame: &mut SyscallFrame) -> SyscallResult {
    crate::proc::thread::Thread::yield_cpu();
    Ok(0)
}

/// `sys_exit` (SYS_EXIT = 60)
/// Terminate the calling thread or process.
pub fn sys_exit(frame: &mut SyscallFrame) -> SyscallResult {
    let code = frame.arg1() as i32;
    log::info!("sys_exit called with status code {}", code);
    Ok(0)
}
