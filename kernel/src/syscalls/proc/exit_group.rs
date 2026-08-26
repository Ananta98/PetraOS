//! sys_exit_group system call handler.

use super::*;
use crate::syscalls::{SyscallResult};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::mm::vmm::paging::PageTable;
use crate::proc::ProcessId;


/// `sys_exit_group` (SYS_EXIT_GROUP = 231)
/// Exit all threads in a process.
pub fn sys_exit_group(frame: &mut SyscallFrame) -> SyscallResult {
    let code = frame.arg1() as i32;
    log::debug!("sys_exit_group called with status code {}", code);
    do_exit(code)
}
