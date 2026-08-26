use super::{SyscallError, SyscallResult};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::mm::{PageTable, PageTableFlags, VirtAddr, VmAreaKind};

// ── Modular syscall submodules ──────────────────────────────────────────
pub mod brk;
pub mod mmap;
pub mod munmap;
pub mod mprotect;

pub use brk::sys_brk;
pub use mmap::sys_mmap;
pub use munmap::sys_munmap;
pub use mprotect::sys_mprotect;

