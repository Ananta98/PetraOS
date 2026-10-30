// SPDX-License-Identifier: GPL-2.0

//! System call `seccomp(2)` (Syscall #317 on x86_64).

use ostd::arch::cpu::context::UserContext;

use crate::uapi::errno::{Errno, Result};

/// System call `seccomp(unsigned int operation, unsigned int flags, void *args)` (Syscall #317).
pub fn sys_seccomp(
    _operation: u32,
    _flags: u32,
    _args_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    // Kept as ENOSYS for now; full seccomp-bpf filter infrastructure will be implemented later.
    Err(Errno::ENOSYS)
}
