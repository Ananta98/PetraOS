// SPDX-License-Identifier: GPL-2.0

//! System calls for writing to file descriptors: `write`, `pwrite64`, `writev`.

use alloc::sync::Arc;

use crate::{
    api::errno::Result,
    fs::vfs::File,
    proc::{Proc, thread::Thread},
    syscall::fs::utils::{IoVec, decode_loffhi, read_user_iovecs},
    utils::userspace::UserPtr,
};
use ostd::arch::cpu::context::UserContext;

/// Per-chunk kernel staging buffer bound for user copy-in.
const IO_CHUNK: usize = 65536;

/// Core of `write`/`pwrite64`: copies user data in 64 KiB chunks and writes
/// until the file takes a short or empty write.
fn do_write(
    proc: &Arc<Proc>,
    file: &Arc<File>,
    buf_ptr: usize,
    count: usize,
    offset: Option<u64>,
) -> Result<usize> {
    let vmar = proc.vmspace();
    let mut total_written = 0;
    let mut kbuf = alloc::vec![0u8; count.min(IO_CHUNK)];

    while total_written < count {
        let to_copy = (count - total_written).min(IO_CHUNK);
        UserPtr::new(buf_ptr + total_written).read_bytes(&vmar, &mut kbuf[..to_copy])?;

        let nwritten = match offset {
            Some(off) => file.pwrite(off + total_written as u64, &kbuf[..to_copy])?,
            None => file.write(&kbuf[..to_copy])?,
        };
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

/// Core of `writev`/`pwritev`: drains each iovec fully (in up-to-64 KiB file
/// writes) before moving to the next, stopping at the first short write.
fn do_writev(
    proc: &Arc<Proc>,
    file: &Arc<File>,
    iovecs: alloc::vec::Vec<IoVec>,
    offset: Option<u64>,
) -> Result<usize> {
    let vmar = proc.vmspace();
    let mut total_written = 0;
    let mut kbuf = alloc::vec![0u8; IO_CHUNK];
    let mut cur_offset = offset.unwrap_or(0);

    for iov in iovecs {
        let mut iov_written = 0;
        while iov_written < iov.iov_len {
            let to_copy = (iov.iov_len - iov_written).min(IO_CHUNK);
            UserPtr::new(iov.iov_base + iov_written).read_bytes(&vmar, &mut kbuf[..to_copy])?;

            let nwritten = match offset {
                Some(_) => file.pwrite(cur_offset, &kbuf[..to_copy])?,
                None => file.write(&kbuf[..to_copy])?,
            };
            if nwritten == 0 || nwritten < to_copy {
                proc.account_write(total_written);
                return Ok(total_written);
            }
            total_written += nwritten;
            iov_written += nwritten;
            cur_offset += nwritten as u64;
        }
    }

    proc.account_write(total_written);
    Ok(total_written)
}

/// System call `write(int fd, const void *buf, size_t count)` (Syscall #1).
pub fn sys_write(fd: i32, buf_ptr: usize, count: usize, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let file = proc.file(fd)?;
    do_write(&proc, &file, buf_ptr, count, None)
}

/// System call `pwrite64(int fd, const void *buf, size_t count, off_t offset)` (Syscall #18).
pub fn sys_pwrite64(
    fd: i32,
    buf_ptr: usize,
    count: usize,
    offset: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let file = proc.file(fd)?;
    do_write(&proc, &file, buf_ptr, count, Some(offset as u64))
}

/// System call `writev(int fd, const struct iovec *iov, int iovcnt)` (Syscall #20).
pub fn sys_writev(fd: i32, iov_ptr: usize, iovcnt: usize, _ctx: &mut UserContext) -> Result<usize> {
    if iovcnt == 0 {
        return Ok(0);
    }
    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();
    let iovecs = read_user_iovecs(&vmar, iov_ptr, iovcnt)?;
    let file = proc.file(fd)?;

    do_writev(&proc, &file, iovecs, None)
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
    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();
    let iovecs = read_user_iovecs(&vmar, iov_ptr, iovcnt)?;
    let file = proc.file(fd)?;

    do_writev(&proc, &file, iovecs, Some(decode_loffhi(pos_l, pos_h)))
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
