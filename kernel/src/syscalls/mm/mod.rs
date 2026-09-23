
// ── Modular syscall submodules ──────────────────────────────────────────
pub mod brk;
pub mod madvise;
pub mod mmap;
pub mod mprotect;
pub mod msync;
pub mod munmap;

pub use brk::sys_brk;
pub use madvise::sys_madvise;
pub use mmap::sys_mmap;
pub use mprotect::sys_mprotect;
pub use msync::sys_msync;
pub use munmap::sys_munmap;

