// SPDX-License-Identifier: GPL-2.0

//! System call `userfaultfd(2)` (Syscall #323 on x86_64).

use ostd::arch::cpu::context::UserContext;

use crate::uapi::errno::{Errno, Result};

/// System call `userfaultfd(int flags)` (Syscall #323).
pub fn sys_userfaultfd(_flags: i32, _ctx: &mut UserContext) -> Result<usize> {
    Err(Errno::ENOSYS)
}
