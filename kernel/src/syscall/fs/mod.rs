pub mod access;
pub mod chdir;
pub mod chmod;
pub mod chown;
pub mod close;
pub mod dir_ops;
pub mod dup;
pub mod dup2;
pub mod dup3;
pub mod epoll;
pub mod eventfd;
pub mod fcntl;
pub mod getdents;
pub mod inotify;
pub mod ioctl;
pub mod lseek;
pub mod mount;
pub mod open;
pub mod pipe;
pub mod poll;
pub mod read;
pub mod stat;
pub mod write;

pub use access::{syscall_access, syscall_faccessat};
pub use chdir::syscall_chdir;
pub use chmod::syscall_chmod;
pub use chown::syscall_chown;
pub use close::syscall_close;
pub use dir_ops::{
    syscall_fstatfs, syscall_getcwd, syscall_mkdir, syscall_mkdirat, syscall_openat,
    syscall_readlink, syscall_readlinkat, syscall_rename, syscall_renameat, syscall_rmdir,
    syscall_statfs, syscall_umask, syscall_unlink, syscall_unlinkat,
};
pub use dup::syscall_dup;
pub use dup2::syscall_dup2;
pub use dup3::syscall_dup3;
pub use epoll::{
    syscall_epoll_create, syscall_epoll_create1, syscall_epoll_ctl, syscall_epoll_pwait,
    syscall_epoll_wait,
};
pub use eventfd::{syscall_eventfd, syscall_eventfd2};
pub use fcntl::syscall_fcntl;
pub use getdents::syscall_getdents64;
pub use inotify::{syscall_inotify_add_watch, syscall_inotify_init1, syscall_inotify_rm_watch};
pub use ioctl::syscall_ioctl;
pub use lseek::syscall_lseek;
pub use mount::syscall_mount;
pub use open::syscall_open;
pub use pipe::syscall_pipe2;
pub use poll::{syscall_poll, syscall_ppoll, syscall_pselect6, syscall_select};
pub use read::syscall_read;
pub use stat::{syscall_fstat, syscall_lstat, syscall_newfstatat, syscall_stat};
pub use write::syscall_write;

use crate::syscall::SyscallResult;
use crate::vm::vma::VmaManager;
use alloc::string::String;
use alloc::vec::Vec;
use ostd::Error;
