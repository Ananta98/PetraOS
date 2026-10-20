// SPDX-License-Identifier: GPL-2.0

//! System calls for reading from file descriptors: `read`, `pread64`, `readv`.

use crate::{
    api::errno::{Errno, Result},
    proc::thread::Thread,
    syscall::fs::utils::read_user_iovecs,
    utils::userspace::UserPtr,
};
use ostd::arch::cpu::context::UserContext;

/// System call `read(int fd, void *buf, size_t count)` (Syscall #0).
pub fn sys_read(fd: i32, buf_ptr: usize, count: usize, _ctx: &mut UserContext) -> Result<usize> {
    if count == 0 {
        return Ok(0);
    }
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let file = proc.fd_table.lock().get(fd)?;

    let mut kbuf = alloc::vec![0u8; count.min(65536)];
    let nread = file.read(&mut kbuf)?;

    if nread > 0 {
        let vmar = proc.vmspace();
        UserPtr::new(buf_ptr).write_bytes(&vmar, &kbuf[..nread])?;
    }
    proc.account_read(nread);

    Ok(nread)
}

/// System call `pread64(int fd, void *buf, size_t count, off_t offset)` (Syscall #17).
pub fn sys_pread64(
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
    let file = proc.fd_table.lock().get(fd)?;

    let mut kbuf = alloc::vec![0u8; count.min(65536)];
    let nread = file.pread(offset as u64, &mut kbuf)?;

    if nread > 0 {
        let vmar = proc.vmspace();
        UserPtr::new(buf_ptr).write_bytes(&vmar, &kbuf[..nread])?;
    }
    proc.account_read(nread);

    Ok(nread)
}

/// System call `readv(int fd, const struct iovec *iov, int iovcnt)` (Syscall #19).
pub fn sys_readv(fd: i32, iov_ptr: usize, iovcnt: usize, _ctx: &mut UserContext) -> Result<usize> {
    if iovcnt == 0 {
        return Ok(0);
    }
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();
    let iovecs = read_user_iovecs(&vmar, iov_ptr, iovcnt)?;
    let file = proc.fd_table.lock().get(fd)?;

    let mut total_read = 0;
    for iov in iovecs {
        if iov.iov_len == 0 {
            continue;
        }
        let chunk_len = iov.iov_len.min(65536);
        let mut kbuf = alloc::vec![0u8; chunk_len];
        let nread = file.read(&mut kbuf)?;
        if nread > 0 {
            UserPtr::new(iov.iov_base).write_bytes(&vmar, &kbuf[..nread])?;
            total_read += nread;
        }
        if nread < chunk_len {
            break;
        }
    }

    proc.account_read(total_read);
    Ok(total_read)
}

/// System call `preadv(int fd, const struct iovec *iov, int iovcnt, off_t pos_l, off_t pos_h)` (Syscall #295).
pub fn sys_preadv(
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

    let mut total_read = 0;
    let mut cur_offset = offset;
    for iov in iovecs {
        if iov.iov_len == 0 {
            continue;
        }
        let chunk_len = iov.iov_len.min(65536);
        let mut kbuf = alloc::vec![0u8; chunk_len];
        let nread = file.pread(cur_offset, &mut kbuf)?;
        if nread > 0 {
            UserPtr::new(iov.iov_base).write_bytes(&vmar, &kbuf[..nread])?;
            total_read += nread;
            cur_offset += nread as u64;
        }
        if nread < chunk_len {
            break;
        }
    }

    proc.account_read(total_read);
    Ok(total_read)
}

/// System call `preadv2(int fd, const struct iovec *iov, int iovcnt, off_t pos_l, off_t pos_h, int flags)` (Syscall #327).
pub fn sys_preadv2(
    fd: i32,
    iov_ptr: usize,
    iovcnt: usize,
    pos_l: usize,
    pos_h: usize,
    _flags: i32,
    ctx: &mut UserContext,
) -> Result<usize> {
    if pos_l == usize::MAX && (pos_h == 0 || pos_h == usize::MAX) {
        return sys_readv(fd, iov_ptr, iovcnt, ctx);
    }
    sys_preadv(fd, iov_ptr, iovcnt, pos_l, pos_h, ctx)
}
