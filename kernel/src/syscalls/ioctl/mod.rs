
// ── Modular syscall submodules ──────────────────────────────────────────
pub mod ioctl;
pub mod isatty;

pub use ioctl::sys_ioctl;
pub use isatty::sys_isatty;


