use super::{SyscallError, SyscallResult};
use crate::fs::vfs::types::VfsError::{BadFd, InvalidInput, NotSupported};
use crate::arch::syscall::syscall::SyscallFrame;

// ── Modular syscall submodules ──────────────────────────────────────────
pub mod ioctl;
pub mod isatty;

pub use ioctl::sys_ioctl;
pub use isatty::sys_isatty;


