//! sys_pselect6 system call handler.

use super::*;
use crate::syscalls::{SyscallResult};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::File;
use crate::fs::vfs::types::{InodeType, LinuxStat, SeekWhence, Stat, StatFs};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;


/// `sys_pselect6` (SYS_PSELECT6 = 270)
pub fn sys_pselect6(frame: &mut SyscallFrame) -> SyscallResult {
    sys_select(frame)
}
