pub(crate) mod close;
pub(crate) mod dup;
pub(crate) mod dup2;
pub(crate) mod lseek;
pub(crate) mod mount;
pub(crate) mod open;
pub(crate) mod pipe;
pub(crate) mod read;
pub(crate) mod write;

pub(crate) use close::syscall_close;
pub(crate) use dup::syscall_dup;
pub(crate) use dup2::syscall_dup2;
pub(crate) use lseek::syscall_lseek;
pub(crate) use mount::syscall_mount;
pub(crate) use open::syscall_open;
pub(crate) use pipe::syscall_pipe2;
pub(crate) use read::syscall_read;
pub(crate) use write::syscall_write;

use crate::syscall::SyscallResult;
use crate::vm::vma::VmaManager;
use alloc::string::String;
use alloc::vec::Vec;
use ostd::Error;
