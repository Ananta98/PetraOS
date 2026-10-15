// SPDX-License-Identifier: GPL-2.0

//! The `pipe` and `pipe2` system calls (Syscalls #22 and #293 on x86_64).

use ostd::arch::cpu::context::UserContext;

use crate::{
    api::errno::{Errno, Result},
    fs::fd::FdFlags,
    proc::thread::Thread,
    syscall::fs::utils::UserPtr,
};

const O_CLOEXEC: i32 = 0o2000000;
const O_NONBLOCK: i32 = 0o4000;

/// System call `pipe2(int pipefd[2], int flags)` (Syscall #293).
pub fn sys_pipe2(
    pipefd_ptr: usize,
    flags: i32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if (flags & !(O_CLOEXEC | O_NONBLOCK)) != 0 {
        return Err(Errno::EINVAL);
    }
    let nonblock = (flags & O_NONBLOCK) != 0;
    let cloexec = if (flags & O_CLOEXEC) != 0 {
        FdFlags::FD_CLOEXEC
    } else {
        FdFlags::empty()
    };

    let (read_end, write_end) = crate::fs::pipe::create_pipe(nonblock);
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let mut table = proc.fd_table.lock();

    let fd0 = table.alloc_fd(read_end, cloexec)?;
    let fd1 = match table.alloc_fd(write_end, cloexec) {
        Ok(fd) => fd,
        Err(e) => {
            let _ = table.close(fd0);
            return Err(e);
        }
    };

    let fds: [i32; 2] = [fd0, fd1];
    let mut bytes = [0u8; 8];
    bytes[0..4].copy_from_slice(&fds[0].to_ne_bytes());
    bytes[4..8].copy_from_slice(&fds[1].to_ne_bytes());

    UserPtr::new(pipefd_ptr).write_bytes(&proc.vmspace(), &bytes)?;
    Ok(0)
}

/// System call `pipe(int pipefd[2])` (Syscall #22).
pub fn sys_pipe(pipefd_ptr: usize, ctx: &mut UserContext) -> Result<usize> {
    sys_pipe2(pipefd_ptr, 0, ctx)
}
