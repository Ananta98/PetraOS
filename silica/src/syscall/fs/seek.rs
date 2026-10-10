// SPDX-License-Identifier: GPL-2.0

//! The `lseek` system call (Syscall #8 on x86_64).

use ostd::arch::cpu::context::UserContext;

use crate::{
    api::errno::{Errno, Result},
    fs::vfs::SeekAnchor,
    proc::thread::Thread,
};

/// Linux whence constants
pub const SEEK_SET: i32 = 0;
pub const SEEK_CUR: i32 = 1;
pub const SEEK_END: i32 = 2;

/// System call `lseek(int fd, off_t offset, int whence)` (Syscall #8).
pub fn sys_lseek(
    fd: i32,
    offset: i64,
    whence: i32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let file = proc.fd_table.lock().get(fd)?;

    let anchor = match whence {
        SEEK_SET => {
            if offset < 0 {
                return Err(Errno::EINVAL);
            }
            SeekAnchor::Start(offset as u64)
        }
        SEEK_CUR => SeekAnchor::Current(offset),
        SEEK_END => SeekAnchor::End(offset),
        _ => return Err(Errno::EINVAL),
    };

    let new_offset = file.seek(anchor)?;
    Ok(new_offset as usize)
}
