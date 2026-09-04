//! System call handler for `arch_prctl` (x86_64 architecture-specific control).


// ── Modular syscall submodules ──────────────────────────────────────────
pub mod arch_prctl;

pub use arch_prctl::sys_arch_prctl;


pub const ARCH_SET_GS: u64 = 0x1001;
pub const ARCH_SET_FS: u64 = 0x1002;
pub const ARCH_GET_FS: u64 = 0x1003;
pub const ARCH_GET_GS: u64 = 0x1004;
