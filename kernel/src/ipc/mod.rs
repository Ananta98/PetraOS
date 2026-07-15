/// Inter-Process Communication (IPC) Subsystem
///
/// Refactored to organize signal-related IPC functionality into a dedicated
/// subdirectory `signal/` for improved maintainability.
///
/// All public symbols are re-exported here to maintain backward compatibility
/// with other kernel subsystems (such as `proc` and `syscall`).
pub mod signal;
pub mod pipe;
pub mod shm;

pub use signal::*;
pub use pipe::create_pipe;
pub use shm::{SHM_REGISTRY, shm_get, shm_at, shm_dt, shm_ctl};
