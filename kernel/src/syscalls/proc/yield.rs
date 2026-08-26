//! sys_yield system call handler.

use super::*;
use crate::syscalls::{SyscallResult};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::mm::vmm::paging::PageTable;
use crate::proc::ProcessId;


/// `sys_yield` (SYS_YIELD = 24)
/// Yield the CPU to another runnable thread.
pub fn sys_yield(_frame: &mut SyscallFrame) -> SyscallResult {
    crate::proc::thread::Thread::yield_cpu();
    Ok(0)
}
