// SPDX-License-Identifier: GPL-2.0

//! System calls for kernel-space file copy: `sendfile(2)`, `copy_file_range(2)`.

use ostd::arch::cpu::context::UserContext;

use crate::{
    api::errno::{Errno, Result},
    proc::thread::Thread,
    syscall::fs::utils::UserPtr,
};

/// System call `sendfile(int out_fd, int in_fd, off_t *offset, size_t count)` (Syscall #40).
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
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();

    let table = proc.fd_table.lock();
    let file_in = table.get(in_fd)?;
    let file_out = table.get(out_fd)?;
    drop(table);

    let (mut offset, has_offset) = if offset_ptr != 0 {
        let mut raw = [0u8; 8];
        UserPtr::new(offset_ptr).read_bytes(&vmar, &mut raw)?;
        (u64::from_ne_bytes(raw), true)
    } else {
        (0, false)
    };

    let mut total_transferred = 0;
    while total_transferred < count {
        let chunk_size = (count - total_transferred).min(65536);
        let mut kbuf = alloc::vec![0u8; chunk_size];

        let nread = if has_offset {
            file_in.pread(offset, &mut kbuf)?
        } else {
            file_in.read(&mut kbuf)?
        };

        if nread == 0 {
            break;
        }

        let nwritten = file_out.write(&kbuf[..nread])?;
        total_transferred += nwritten;
        if has_offset {
            offset += nwritten as u64;
        }

        if nwritten < nread {
            break;
        }
    }

    if has_offset {
        UserPtr::new(offset_ptr).write_bytes(&vmar, &offset.to_ne_bytes())?;
    }

    Ok(total_transferred)
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
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();

    let table = proc.fd_table.lock();
    let file_in = table.get(fd_in)?;
    let file_out = table.get(fd_out)?;
    drop(table);

    let (mut off_in, has_off_in) = if off_in_ptr != 0 {
        let mut raw = [0u8; 8];
        UserPtr::new(off_in_ptr).read_bytes(&vmar, &mut raw)?;
        (u64::from_ne_bytes(raw), true)
    } else {
        (0, false)
    };

    let (mut off_out, has_off_out) = if off_out_ptr != 0 {
        let mut raw = [0u8; 8];
        UserPtr::new(off_out_ptr).read_bytes(&vmar, &mut raw)?;
        (u64::from_ne_bytes(raw), true)
    } else {
        (0, false)
    };

    let mut total_copied = 0;
    while total_copied < len {
        let chunk_size = (len - total_copied).min(65536);
        let mut kbuf = alloc::vec![0u8; chunk_size];

        let nread = if has_off_in {
            file_in.pread(off_in, &mut kbuf)?
        } else {
            file_in.read(&mut kbuf)?
        };

        if nread == 0 {
            break;
        }

        let nwritten = if has_off_out {
            file_out.pwrite(off_out, &kbuf[..nread])?
        } else {
            file_out.write(&kbuf[..nread])?
        };

        total_copied += nwritten;
        if has_off_in {
            off_in += nwritten as u64;
        }
        if has_off_out {
            off_out += nwritten as u64;
        }

        if nwritten < nread {
            break;
        }
    }

    if has_off_in {
        UserPtr::new(off_in_ptr).write_bytes(&vmar, &off_in.to_ne_bytes())?;
    }
    if has_off_out {
        UserPtr::new(off_out_ptr).write_bytes(&vmar, &off_out.to_ne_bytes())?;
    }

    Ok(total_copied)
}
