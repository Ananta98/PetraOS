//! System call handler for `arch_prctl` (x86_64 architecture-specific control).

// ── Modular syscall submodules ──────────────────────────────────────────
pub mod arch_prctl;

pub use arch_prctl::*;

