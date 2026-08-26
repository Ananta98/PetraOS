//! sys_vfork system call handler.

use super::*;
use crate::syscalls::{SyscallResult};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::mm::vmm::paging::PageTable;
use crate::proc::ProcessId;


/// `sys_vfork` (SYS_VFORK = 58)
/// Create a child process and block parent until exec/exit.
pub fn sys_vfork(frame: &mut SyscallFrame) -> SyscallResult {
    sys_fork(frame)
}
