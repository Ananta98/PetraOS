// SPDX-License-Identifier: GPL-2.0

//! System call `bpf(2)` (Syscall #321 on x86_64).

use ostd::arch::cpu::context::UserContext;

use crate::uapi::errno::{Errno, Result};

/// System call `bpf(int cmd, union bpf_attr *attr, unsigned int size)` (Syscall #321).
pub fn sys_bpf(_cmd: i32, _attr_ptr: usize, _size: u32, _ctx: &mut UserContext) -> Result<usize> {
    Err(Errno::ENOSYS)
}
