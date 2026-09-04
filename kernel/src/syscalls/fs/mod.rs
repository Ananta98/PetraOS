//! Filesystem system call handlers and common abstractions.

pub mod access;
pub mod chdir;
pub mod chmod;
pub mod chown;
pub mod close;
pub mod dup;
pub mod fadvise;
pub mod fcntl;
pub mod fsync;
pub mod getdents;
pub mod link;
pub mod lseek;
pub mod mkdir;
pub mod mknod;
pub mod open;
pub mod pipe;
pub mod poll;
pub mod read;
pub mod rename;
pub mod stat;
pub mod statfs;
pub mod truncate;
pub mod umask;
pub mod unlink;
pub mod utimensat;
pub mod write;

// ── Re-exports of system call entry points ──────────────────────────────────
pub use access::{sys_access, sys_faccessat};
pub use chdir::{sys_chdir, sys_fchdir, sys_getcwd};
pub use chmod::{sys_chmod, sys_fchmod, sys_fchmodat};
pub use chown::{sys_chown, sys_fchown, sys_fchownat, sys_lchown};
pub use close::sys_close;
pub use dup::{sys_dup, sys_dup2, sys_dup3};
pub use fadvise::sys_fadvise64;
pub use fcntl::{sys_fcntl, sys_flock};
pub use fsync::{sys_fdatasync, sys_fsync};
pub use getdents::sys_getdents64;
pub use link::{sys_link, sys_linkat, sys_readlink, sys_readlinkat, sys_symlink, sys_symlinkat};
pub use lseek::sys_lseek;
pub use mkdir::{sys_mkdir, sys_mkdirat, sys_rmdir};
pub use mknod::sys_mknodat;
pub use open::{sys_open, sys_openat};
pub use pipe::{sys_pipe, sys_pipe2};
pub use poll::{
    sys_poll, sys_ppoll, sys_pselect6, sys_select, PollFd, POLLERR, POLLHUP, POLLIN, POLLNVAL,
    POLLOUT, POLLPRI,
};
pub use read::{sys_pread64, sys_read, sys_readv};
pub use rename::{sys_rename, sys_renameat, sys_renameat2};
pub use stat::{sys_fstat, sys_lstat, sys_newfstatat, sys_stat};
pub use statfs::{sys_fstatfs, sys_statfs};
pub use truncate::{sys_ftruncate, sys_truncate};
pub use umask::sys_umask;
pub use unlink::{sys_unlink, sys_unlinkat};
pub use utimensat::{sys_futimesat, sys_utimensat, LinuxTimespec};
pub use write::{sys_pwrite64, sys_write, sys_writev};

// ── Common filesystem syscall types & constants ─────────────────────────────
use crate::syscalls::SyscallError;
use alloc::string::String;

pub const AT_FDCWD: i32 = -100;

pub const O_CLOEXEC: u32 = 0x80000;
pub const O_NONBLOCK: u32 = 0x800;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct IoVec {
    pub iov_base: u64,
    pub iov_len: usize,
}

/// Resolve a pathname against a directory file descriptor `dfd`.
pub fn resolve_at_path(dfd: i32, path: &str) -> Result<String, SyscallError> {
    if path.starts_with('/') {
        Ok(crate::fs::normalize_path("/", path))
    } else if dfd == AT_FDCWD || dfd as u32 == 0xffffff9c {
        let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
        let proc = proc_arc.lock();
        Ok(crate::fs::normalize_path(&proc.cwd, path))
    } else if dfd >= 0 {
        let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
        let proc = proc_arc.lock();
        let file = proc.fd_table.get(dfd)?;
        let dir_path = crate::fs::build_path(&file.dentry);
        Ok(crate::fs::normalize_path(&dir_path, path))
    } else {
        let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
        let proc = proc_arc.lock();
        Ok(crate::fs::normalize_path(&proc.cwd, path))
    }
}
