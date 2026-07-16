pub mod pipe;
pub mod shm;
/// Inter-Process Communication (IPC) Subsystem
///
/// Refactored to organize signal-related IPC functionality into a dedicated
/// subdirectory `signal/` for improved maintainability.
///
/// All public symbols are re-exported here to maintain backward compatibility
/// with other kernel subsystems (such as `proc` and `syscall`).
pub mod signal;

pub use pipe::create_pipe;
pub use shm::{SHM_REGISTRY, shm_at, shm_ctl, shm_dt, shm_get};
pub use signal::*;
