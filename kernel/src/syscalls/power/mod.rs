//! Power management system call handlers.
//!
//! Provides support for:
//! - [`reboot`]: Linux-compatible `reboot(2)` handler (SYS_REBOOT = 169).

pub mod reboot;

pub use reboot::sys_reboot;
