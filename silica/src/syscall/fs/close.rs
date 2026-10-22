// SPDX-License-Identifier: GPL-2.0

//! The `close` system call (Syscall #3 on x86_64).

use ostd::arch::cpu::context::UserContext;

use crate::{proc::thread::Thread, uapi::errno::Result};

/// System call `close(int fd)` (Syscall #3).
pub fn sys_close(fd: i32, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    proc.fd_table.lock().close(fd)?;
    Ok(0)
}
