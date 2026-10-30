// SPDX-License-Identifier: GPL-2.0

//! System calls `unshare(2)` and `setns(2)` (Syscalls #272 and #308 on x86_64).

use ostd::arch::cpu::context::UserContext;

use crate::uapi::errno::{Errno, Result};

/// System call `unshare(int flags)` (Syscall #272).
pub fn sys_unshare(_flags: i32, _ctx: &mut UserContext) -> Result<usize> {
    Err(Errno::ENOSYS)
}

/// System call `setns(int fd, int nstype)` (Syscall #308).
pub fn sys_setns(_fd: i32, _nstype: i32, _ctx: &mut UserContext) -> Result<usize> {
    Err(Errno::ENOSYS)
}
