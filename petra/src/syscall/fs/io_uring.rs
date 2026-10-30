// SPDX-License-Identifier: GPL-2.0

//! System calls `io_uring_setup(2)`, `io_uring_enter(2)`, and `io_uring_register(2)`
//! (Syscalls #425, #426, and #427 on x86_64).

use ostd::arch::cpu::context::UserContext;

use crate::uapi::errno::{Errno, Result};

/// System call `io_uring_setup(u32 entries, struct io_uring_params *params)` (Syscall #425).
pub fn sys_io_uring_setup(_entries: u32, _params_ptr: usize, _ctx: &mut UserContext) -> Result<usize> {
    Err(Errno::ENOSYS)
}

/// System call `io_uring_enter(unsigned int fd, u32 to_submit, u32 min_complete, u32 flags, sigset_t *sig, size_t sigsz)` (Syscall #426).
pub fn sys_io_uring_enter(
    _fd: u32,
    _to_submit: u32,
    _min_complete: u32,
    _flags: u32,
    _sig: usize,
    _sigsz: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    Err(Errno::ENOSYS)
}

/// System call `io_uring_register(unsigned int fd, unsigned int opcode, void *arg, unsigned int nr_args)` (Syscall #427).
pub fn sys_io_uring_register(
    _fd: u32,
    _opcode: u32,
    _arg_ptr: usize,
    _nr_args: u32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    Err(Errno::ENOSYS)
}
