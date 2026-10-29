// SPDX-License-Identifier: GPL-2.0

//! System call `fallocate` (Syscall #285 on x86_64).

use ostd::arch::cpu::context::UserContext;

use crate::{
    uapi::errno::{Errno, Result},
    fs::vfs::{File, OpenFlags},
    proc::thread::Thread,
};

/// Do not change the file size, even if the allocated range extends past EOF.
const FALLOC_FL_KEEP_SIZE: i32 = 0x01;
/// Deallocate the range, replacing it with a hole.
const FALLOC_FL_PUNCH_HOLE: i32 = 0x02;
/// Zero-fill the range without deallocating it.
const FALLOC_FL_ZERO_RANGE: i32 = 0x10;

/// Mode bits this implementation understands. Range-shifting modes such as
/// `FALLOC_FL_COLLAPSE_RANGE` are rejected with `EOPNOTSUPP`.
const SUPPORTED_FLAGS: i32 = FALLOC_FL_KEEP_SIZE | FALLOC_FL_PUNCH_HOLE | FALLOC_FL_ZERO_RANGE;

/// Chunk size used when zero-filling a range, to bound stack use.
const ZERO_CHUNK: usize = 4096;

/// System call `fallocate(int fd, int mode, off_t offset, off_t len)` (Syscall #285).
pub fn sys_fallocate(
    fd: i32,
    mode: i32,
    offset: i64,
    len: i64,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if offset < 0 || len <= 0 {
        return Err(Errno::EINVAL);
    }
    if mode & !SUPPORTED_FLAGS != 0 {
        return Err(Errno::EOPNOTSUPP);
    }
    // Hole punching must not change the file size.
    if mode & FALLOC_FL_PUNCH_HOLE != 0 && mode & FALLOC_FL_KEEP_SIZE == 0 {
        return Err(Errno::EINVAL);
    }

    let proc = Thread::proc_or_esrch()?;
    let file = proc.file(fd)?;

    if !file.flags.lock().contains(OpenFlags::WRITE) {
        return Err(Errno::EBADF);
    }
    if file.inode.is_dir() {
        return Err(Errno::EISDIR);
    }
    if !file.inode.is_file() {
        return Err(Errno::ENODEV);
    }

    let offset = offset as usize;
    let len = len as usize;
    let end = offset.checked_add(len).ok_or(Errno::EFBIG)?;

    let keep_size = mode & FALLOC_FL_KEEP_SIZE != 0;
    let size = file.inode.size();

    if mode & (FALLOC_FL_PUNCH_HOLE | FALLOC_FL_ZERO_RANGE) != 0 {
        // A hole past EOF is a no-op, so punching never zero-fills beyond the
        // current size; a zero range may extend the file.
        let zero_end = if mode & FALLOC_FL_PUNCH_HOLE != 0 {
            end.min(size)
        } else {
            end
        };
        if zero_end > offset {
            zero_range(&file, offset, zero_end - offset)?;
        }
    }

    if !keep_size && end > size {
        let mut attr = *file.inode.attr.read();
        attr.size = end;
        file.inode.node_ops.setattr(&attr)?;
        file.inode.set_size(end);
        file.inode.touch_mtime();
    }

    Ok(0)
}

/// Writes zeros over `[offset, offset + len)` in bounded chunks.
fn zero_range(file: &File, offset: usize, len: usize) -> Result<()> {
    let zeros = [0u8; ZERO_CHUNK];
    let mut written = 0;
    while written < len {
        let chunk = (len - written).min(ZERO_CHUNK);
        file.pwrite((offset + written) as u64, &zeros[..chunk])?;
        written += chunk;
    }
    Ok(())
}
