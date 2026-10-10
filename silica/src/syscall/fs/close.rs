// SPDX-License-Identifier: GPL-2.0

//! The `close` system call (Syscall #3 on x86_64).

use ostd::arch::cpu::context::UserContext;

use crate::{
    api::errno::{Errno, Result},
    proc::thread::Thread,
};

/// System call `close(int fd)` (Syscall #3).
pub fn sys_close(fd: i32, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    proc.fd_table.lock().close(fd)?;
    Ok(0)
}
