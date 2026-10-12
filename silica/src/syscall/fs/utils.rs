// SPDX-License-Identifier: GPL-2.0

//! Shared utilities for filesystem system calls.

use alloc::{sync::Arc, vec::Vec};

pub use crate::utils::userspace::{
    user_cstr::read_user_string,
    user_ptr::{UserPtr, read_user_bytes, write_user_bytes},
};

use crate::{
    api::errno::{Errno, Result},
    fs::{
        fd::FdFlags,
        vfs::{OpenFlags, PathNode},
    },
    proc::Proc,
    vm::vmar::Vmar,
};

pub const AT_FDCWD: i32 = -100;
pub const AT_SYMLINK_NOFOLLOW: i32 = 0x100;
pub const AT_REMOVEDIR: i32 = 0x200;
pub const AT_SYMLINK_FOLLOW: i32 = 0x400;
pub const AT_NO_AUTOMOUNT: i32 = 0x800;
pub const AT_EMPTY_PATH: i32 = 0x1000;

/// Resolves the starting PathNode for an *at system call.
pub fn resolve_at_start(proc: &Arc<Proc>, dirfd: i32, path: &str) -> Result<PathNode> {
    if path.starts_with('/') {
        return crate::fs::root();
    }
    if dirfd == AT_FDCWD {
        return proc.cwd();
    }
    if dirfd < 0 {
        return Err(Errno::EBADF);
    }
    let file = proc.fd_table.lock().get(dirfd)?;
    if !file.inode.is_dir() {
        return Err(Errno::ENOTDIR);
    }
    file.path_node.clone().ok_or(Errno::EBADF)
}

/// Converts Linux open flags into PetraOS (OpenFlags, FdFlags).
pub fn parse_open_flags(flags: i32) -> (OpenFlags, FdFlags) {
    let mut open_flags = OpenFlags::empty();
    let mut fd_flags = FdFlags::empty();

    match flags & 3 {
        0 => open_flags |= OpenFlags::READ,
        1 => open_flags |= OpenFlags::WRITE,
        2 => open_flags |= OpenFlags::READ | OpenFlags::WRITE,
        _ => {}
    }

    if (flags & 0o100) != 0 {
        open_flags |= OpenFlags::CREATE;
    }
    if (flags & 0o200) != 0 {
        open_flags |= OpenFlags::EXCL;
    }
    if (flags & 0o1000) != 0 {
        open_flags |= OpenFlags::TRUNC;
    }
    if (flags & 0o2000) != 0 {
        open_flags |= OpenFlags::APPEND;
    }
    if (flags & 0o4000) != 0 {
        open_flags |= OpenFlags::NONBLOCK;
    }
    if (flags & 0o200000) != 0 {
        open_flags |= OpenFlags::DIRECTORY;
    }
    if (flags & 0o2000000) != 0 {
        open_flags |= OpenFlags::CLOEXEC;
        fd_flags |= FdFlags::FD_CLOEXEC;
    }

    (open_flags, fd_flags)
}

/// Linux `struct iovec` describing a single buffer in scatter/gather I/O.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IoVec {
    pub iov_base: usize,
    pub iov_len: usize,
}

/// Safely copies an array of `struct iovec` from user memory.
pub fn read_user_iovecs(vmar: &Vmar, iov_ptr: usize, iovcnt: usize) -> Result<Vec<IoVec>> {
    if iovcnt > 1024 {
        return Err(Errno::EINVAL);
    }
    if iovcnt == 0 {
        return Ok(Vec::new());
    }
    let total_bytes = iovcnt
        .checked_mul(core::mem::size_of::<IoVec>())
        .ok_or(Errno::EINVAL)?;
    let mut raw = alloc::vec![0u8; total_bytes];
    UserPtr::new(iov_ptr).read_bytes(vmar, &mut raw)?;

    let mut iovecs = Vec::with_capacity(iovcnt);
    for chunk in raw.chunks_exact(core::mem::size_of::<IoVec>()) {
        let base = usize::from_ne_bytes(chunk[0..8].try_into().unwrap());
        let len = usize::from_ne_bytes(chunk[8..16].try_into().unwrap());
        iovecs.push(IoVec {
            iov_base: base,
            iov_len: len,
        });
    }
    Ok(iovecs)
}
