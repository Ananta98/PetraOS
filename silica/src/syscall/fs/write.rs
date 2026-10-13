// SPDX-License-Identifier: GPL-2.0

//! System calls for writing to file descriptors: `write`, `pwrite64`, `writev`.

use ostd::arch::cpu::context::UserContext;
use crate::{
    api::errno::{Errno, Result},
    proc::thread::Thread,
    syscall::fs::utils::read_user_iovecs,
    utils::userspace::UserPtr,
};

/// System call `write(int fd, const void *buf, size_t count)` (Syscall #1).
pub fn sys_write(fd: i32, buf_ptr: usize, count: usize, _ctx: &mut UserContext) -> Result<usize> {
    if count == 0 {
        return Ok(0);
    }
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();

    let mut kbuf = alloc::vec![0u8; count.min(65536)];
    let copy_len = kbuf.len();

    UserPtr::new(buf_ptr).read_bytes(&vmar, &mut kbuf[..copy_len])?;

    let file = proc.fd_table.lock().get(fd)?;
    file.write(&kbuf[..copy_len])
}

/// System call `pwrite64(int fd, const void *buf, size_t count, off_t offset)` (Syscall #18).
pub fn sys_pwrite64(
    fd: i32,
    buf_ptr: usize,
    count: usize,
    offset: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if count == 0 {
        return Ok(0);
    }
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();

    let mut kbuf = alloc::vec![0u8; count.min(65536)];
    let copy_len = kbuf.len();

    UserPtr::new(buf_ptr).read_bytes(&vmar, &mut kbuf[..copy_len])?;

    let file = proc.fd_table.lock().get(fd)?;
    file.pwrite(offset as u64, &kbuf[..copy_len])
}

/// System call `writev(int fd, const struct iovec *iov, int iovcnt)` (Syscall #20).
pub fn sys_writev(fd: i32, iov_ptr: usize, iovcnt: usize, _ctx: &mut UserContext) -> Result<usize> {
    if iovcnt == 0 {
        return Ok(0);
    }
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();
    let iovecs = read_user_iovecs(&vmar, iov_ptr, iovcnt)?;
    let file = proc.fd_table.lock().get(fd)?;

    let mut total_written = 0;
    for iov in iovecs {
        if iov.iov_len == 0 {
            continue;
        }
        let chunk_len = iov.iov_len.min(65536);
        let mut kbuf = alloc::vec![0u8; chunk_len];

        UserPtr::new(iov.iov_base).read_bytes(&vmar, &mut kbuf[..chunk_len])?;

        let nwritten = file.write(&kbuf[..chunk_len])?;
        total_written += nwritten;
        if nwritten < chunk_len {
            break;
        }
    }

    Ok(total_written)
}

/// System call `pwritev(int fd, const struct iovec *iov, int iovcnt, off_t pos_l, off_t pos_h)` (Syscall #296).
pub fn sys_pwritev(
    fd: i32,
    iov_ptr: usize,
    iovcnt: usize,
    pos_l: usize,
    pos_h: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if iovcnt == 0 {
        return Ok(0);
    }
    let offset = if pos_h != 0 {
        ((pos_h as u64) << 32) | (pos_l as u64)
    } else {
        pos_l as u64
    };

    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();
    let iovecs = read_user_iovecs(&vmar, iov_ptr, iovcnt)?;
    let file = proc.fd_table.lock().get(fd)?;

    let mut total_written = 0;
    let mut cur_offset = offset;
    for iov in iovecs {
        if iov.iov_len == 0 {
            continue;
        }
        let chunk_len = iov.iov_len.min(65536);
        let mut kbuf = alloc::vec![0u8; chunk_len];

        UserPtr::new(iov.iov_base).read_bytes(&vmar, &mut kbuf[..chunk_len])?;

        let nwritten = file.pwrite(cur_offset, &kbuf[..chunk_len])?;
        total_written += nwritten;
        cur_offset += nwritten as u64;
        if nwritten < chunk_len {
            break;
        }
    }

    Ok(total_written)
}

/// System call `pwritev2(int fd, const struct iovec *iov, int iovcnt, off_t pos_l, off_t pos_h, int flags)` (Syscall #328).
pub fn sys_pwritev2(
    fd: i32,
    iov_ptr: usize,
    iovcnt: usize,
    pos_l: usize,
    pos_h: usize,
    _flags: i32,
    ctx: &mut UserContext,
) -> Result<usize> {
    if pos_l == usize::MAX && (pos_h == 0 || pos_h == usize::MAX) {
        return sys_writev(fd, iov_ptr, iovcnt, ctx);
    }
    sys_pwritev(fd, iov_ptr, iovcnt, pos_l, pos_h, ctx)
}

