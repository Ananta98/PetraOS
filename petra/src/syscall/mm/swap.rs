// SPDX-License-Identifier: GPL-2.0

//! System calls `swapon(2)` and `swapoff(2)` (Syscalls #167 and #168 on x86_64).

use ostd::arch::cpu::context::UserContext;

use crate::uapi::errno::{Errno, Result};

/// System call `swapon(const char *path, int swapflags)` (Syscall #167).
pub fn sys_swapon(_path_ptr: usize, _swapflags: i32, _ctx: &mut UserContext) -> Result<usize> {
    Err(Errno::ENOSYS)
}

/// System call `swapoff(const char *path)` (Syscall #168).
pub fn sys_swapoff(_path_ptr: usize, _ctx: &mut UserContext) -> Result<usize> {
    Err(Errno::ENOSYS)
}
