// SPDX-License-Identifier: GPL-2.0

//! System calls for writing to file descriptors: `write`, `pwrite64`, `writev`.

use crate::{
    api::errno::{Errno, Result},
    proc::thread::Thread,
    syscall::fs::utils::read_user_iovecs,
    utils::userspace::UserPtr,
};
use ostd::arch::cpu::context::UserContext;

/// System call `write(int fd, const void *buf, size_t count)` (Syscall #1).
pub fn sys_write(fd: i32, buf_ptr: usize, count: usize, _ctx: &mut UserContext) -> Result<usize> {
    if count == 0 {
        return Ok(0);
    }
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();
    let file = proc.fd_table.lock().get(fd)?;

    let mut total_written = 0;
    let chunk_size = 65536;
    let mut kbuf = alloc::vec![0u8; count.min(chunk_size)];

    while total_written < count {
        let to_copy = (count - total_written).min(chunk_size);
        UserPtr::new(buf_ptr + total_written).read_bytes(&vmar, &mut kbuf[..to_copy])?;

        let nwritten = file.write(&kbuf[..to_copy])?;
        if nwritten == 0 {
            break;
        }
        total_written += nwritten;
        if nwritten < to_copy {
            break;
        }
    }

    proc.account_write(total_written);
    Ok(total_written)
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
    let file = proc.fd_table.lock().get(fd)?;

    let mut total_written = 0;
    let chunk_size = 65536;
    let mut kbuf = alloc::vec![0u8; count.min(chunk_size)];

    while total_written < count {
        let to_copy = (count - total_written).min(chunk_size);
        UserPtr::new(buf_ptr + total_written).read_bytes(&vmar, &mut kbuf[..to_copy])?;

        let nwritten = file.pwrite((offset + total_written) as u64, &kbuf[..to_copy])?;
        if nwritten == 0 {
            break;
        }
        total_written += nwritten;
        if nwritten < to_copy {
            break;
        }
    }

    proc.account_write(total_written);
    Ok(total_written)
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
    let chunk_size = 65536;
    let mut kbuf = alloc::vec![0u8; chunk_size];

    for iov in iovecs {
        let mut iov_written = 0;
        while iov_written < iov.iov_len {
            let to_copy = (iov.iov_len - iov_written).min(chunk_size);
            UserPtr::new(iov.iov_base + iov_written).read_bytes(&vmar, &mut kbuf[..to_copy])?;

            let nwritten = file.write(&kbuf[..to_copy])?;
            if nwritten == 0 {
                proc.account_write(total_written);
                return Ok(total_written);
            }
            total_written += nwritten;
            iov_written += nwritten;
            if nwritten < to_copy {
                proc.account_write(total_written);
                return Ok(total_written);
            }
        }
    }

    proc.account_write(total_written);
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
    let chunk_size = 65536;
    let mut kbuf = alloc::vec![0u8; chunk_size];

    for iov in iovecs {
        let mut iov_written = 0;
        while iov_written < iov.iov_len {
            let to_copy = (iov.iov_len - iov_written).min(chunk_size);
            UserPtr::new(iov.iov_base + iov_written).read_bytes(&vmar, &mut kbuf[..to_copy])?;

            let nwritten = file.pwrite(cur_offset, &kbuf[..to_copy])?;
            if nwritten == 0 {
                proc.account_write(total_written);
                return Ok(total_written);
            }
            total_written += nwritten;
            iov_written += nwritten;
            cur_offset += nwritten as u64;
            if nwritten < to_copy {
                proc.account_write(total_written);
                return Ok(total_written);
            }
        }
    }

    proc.account_write(total_written);
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
