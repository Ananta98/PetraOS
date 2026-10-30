// SPDX-License-Identifier: GPL-2.0

//! System calls `pidfd_send_signal(2)`, `pidfd_open(2)`, and `pidfd_getfd(2)`
//! (Syscalls #424, #434, and #438 on x86_64).

use ostd::arch::cpu::context::UserContext;

use crate::uapi::errno::{Errno, Result};

/// System call `pidfd_send_signal(int pidfd, int sig, siginfo_t *info, unsigned int flags)` (Syscall #424).
pub fn sys_pidfd_send_signal(
    _pidfd: i32,
    _sig: i32,
    _info_ptr: usize,
    _flags: u32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    Err(Errno::ENOSYS)
}

/// System call `pidfd_open(pid_t pid, unsigned int flags)` (Syscall #434).
pub fn sys_pidfd_open(_pid: i32, _flags: u32, _ctx: &mut UserContext) -> Result<usize> {
    Err(Errno::ENOSYS)
}

/// System call `pidfd_getfd(int pidfd, int targetfd, unsigned int flags)` (Syscall #438).
pub fn sys_pidfd_getfd(
    _pidfd: i32,
    _targetfd: i32,
    _flags: u32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    Err(Errno::ENOSYS)
}
