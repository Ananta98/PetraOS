// SPDX-License-Identifier: GPL-2.0

//! System calls for kernel-space file copy: `sendfile(2)`, `copy_file_range(2)`.

use alloc::sync::Arc;
use ostd::arch::cpu::context::UserContext;

use crate::{
    uapi::errno::Result,
    fs::vfs::File,
    proc::{Proc, thread::Thread},
    syscall::fs::utils::UserPtr,
};

/// Per-chunk kernel staging buffer bound for the copy pump.
const COPY_CHUNK: usize = 65536;

/// Shared pump of `sendfile`/`copy_file_range`: copies in 64 KiB chunks,
/// stopping when the input hits EOF or the output takes a short write.
///
/// `off_in`/`off_out` carry the caller's starting position; `None` streams
/// through the file descriptor cursor like plain `read`/`write`.
fn copy_pump(
    file_in: &Arc<File>,
    file_out: &Arc<File>,
    count: usize,
    off_in: Option<u64>,
    off_out: Option<u64>,
) -> Result<usize> {
    let mut off_in = off_in;
    let mut off_out = off_out;

    let mut total_transferred = 0;
    while total_transferred < count {
        let chunk_size = (count - total_transferred).min(COPY_CHUNK);
        let mut kbuf = alloc::vec![0u8; chunk_size];

        let nread = match off_in.as_mut() {
            Some(off) => file_in.pread(*off, &mut kbuf)?,
            None => file_in.read(&mut kbuf)?,
        };
        if nread == 0 {
            break;
        }

        let nwritten = match off_out.as_mut() {
            Some(off) => file_out.pwrite(*off, &kbuf[..nread])?,
            None => file_out.write(&kbuf[..nread])?,
        };
        total_transferred += nwritten;
        if let Some(off) = off_in.as_mut() {
            *off += nwritten as u64;
        }
        if let Some(off) = off_out.as_mut() {
            *off += nwritten as u64;
        }

        if nwritten < nread {
            break;
        }
    }
    Ok(total_transferred)
}

/// Reads the optional positional offsets from user space, runs `copy_pump`,
/// and reports the advanced values back (`start + transferred`) when the
/// caller passed the pointers.
fn copy_with_offsets(
    proc: &Arc<Proc>,
    file_in: &Arc<File>,
    file_out: &Arc<File>,
    count: usize,
    off_in_ptr: usize,
    off_out_ptr: usize,
) -> Result<usize> {
    let vmar = proc.vmspace();
    let start_in: Option<u64> = UserPtr::new(off_in_ptr).read_opt(&vmar)?;
    let start_out: Option<u64> = UserPtr::new(off_out_ptr).read_opt(&vmar)?;

    let transferred = copy_pump(file_in, file_out, count, start_in, start_out)?;

    if let Some(start) = start_in {
        UserPtr::new(off_in_ptr).write_val(&vmar, start + transferred as u64)?;
    }
    if let Some(start) = start_out {
        UserPtr::new(off_out_ptr).write_val(&vmar, start + transferred as u64)?;
    }
    Ok(transferred)
}

/// System call `sendfile(int out_fd, int in_fd, off_t *offset, size_t count)` (Syscall #40).
///
/// The output always streams through its descriptor cursor; only the input
/// position is caller-supplied, so the absent out-offset is encoded as a
/// pointer the call never touches.
pub fn sys_sendfile(
    out_fd: i32,
    in_fd: i32,
    offset_ptr: usize,
    count: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if count == 0 {
        return Ok(0);
    }
    let proc = Thread::proc_or_esrch()?;

    let table = proc.fd_table.lock();
    let file_in = table.get(in_fd)?;
    let file_out = table.get(out_fd)?;
    drop(table);

    copy_with_offsets(&proc, &file_in, &file_out, count, offset_ptr, 0)
}

/// System call `copy_file_range(int fd_in, loff_t *off_in, int fd_out, loff_t *off_out, size_t len, unsigned int flags)` (Syscall #326).
pub fn sys_copy_file_range(
    fd_in: i32,
    off_in_ptr: usize,
    fd_out: i32,
    off_out_ptr: usize,
    len: usize,
    _flags: u32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if len == 0 {
        return Ok(0);
    }
    let proc = Thread::proc_or_esrch()?;

    let table = proc.fd_table.lock();
    let file_in = table.get(fd_in)?;
    let file_out = table.get(fd_out)?;
    drop(table);

    copy_with_offsets(&proc, &file_in, &file_out, len, off_in_ptr, off_out_ptr)
}
