//! sys_exit system call handler.

use super::*;
use crate::syscalls::{SyscallResult};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::mm::vmm::paging::PageTable;
use crate::proc::ProcessId;


/// `sys_exit` (SYS_EXIT = 60)
/// Terminate the calling thread or process.
pub fn sys_exit(frame: &mut SyscallFrame) -> SyscallResult {
    let code = frame.arg1() as i32;
    log::debug!("sys_exit called with status code {}", code);
    do_exit(code)
}
