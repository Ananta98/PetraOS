//! Power Management Subsystem for x86_64 Architecture.
//!
//! Exposes centralized interfaces for machine power state transitions:
//! - System Poweroff / Shutdown (`poweroff`)
//! - System Reboot (`reboot`)
//! - CPU Idle and Halt operations (`idle`)

pub mod reboot;
pub mod shutdown;

pub use reboot::reboot;
pub use shutdown::poweroff;

// Re-export existing CPU power control helpers (adhering to DRY)
pub use crate::arch::cpu::control::idle;
