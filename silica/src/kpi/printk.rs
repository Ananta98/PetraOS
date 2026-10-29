// SPDX-License-Identifier: GPL-2.0

//! Linux-compatible kernel printing macros and error helpers.
//!
//! Provides standard Linux driver logging facilities:
//! - Subsystem prints: `pr_info!`, `pr_warn!`, `pr_err!`, `pr_debug!`.
//! - Device prints: `dev_info!`, `dev_warn!`, `dev_err!`, `dev_debug!`.
//! - Standard Linux errno re-exports (`ENODEV`, `EINVAL`, `ENOMEM`, `EBUSY`, `EIO`, etc.).

pub use crate::uapi::errno::{Errno, Result};

/// Standard Linux error numbers.
pub const EPERM: Errno = Errno::EPERM;
pub const ENOENT: Errno = Errno::ENOENT;
pub const EIO: Errno = Errno::EIO;
pub const ENXIO: Errno = Errno::ENXIO;
pub const EAGAIN: Errno = Errno::EAGAIN;
pub const ENOMEM: Errno = Errno::ENOMEM;
pub const EACCES: Errno = Errno::EACCES;
pub const EFAULT: Errno = Errno::EFAULT;
pub const EBUSY: Errno = Errno::EBUSY;
pub const EEXIST: Errno = Errno::EEXIST;
pub const ENODEV: Errno = Errno::ENODEV;
pub const EINVAL: Errno = Errno::EINVAL;
pub const ENOTTY: Errno = Errno::ENOTTY;
pub const ENOSPC: Errno = Errno::ENOSPC;

/// Linux `pr_info`: informative kernel print.
#[macro_export]
macro_rules! pr_info {
    ($($arg:tt)*) => {
        ostd::info!($($arg)*)
    };
}

/// Linux `pr_warn`: warning kernel print.
#[macro_export]
macro_rules! pr_warn {
    ($($arg:tt)*) => {
        ostd::warn!($($arg)*)
    };
}

/// Linux `pr_err`: error kernel print.
#[macro_export]
macro_rules! pr_err {
    ($($arg:tt)*) => {
        ostd::error!($($arg)*)
    };
}

/// Linux `pr_debug`: debug kernel print.
#[macro_export]
macro_rules! pr_debug {
    ($($arg:tt)*) => {
        // Mapped to trace level in OSTD
        ostd::trace!($($arg)*)
    };
}

/// Linux `dev_info`: device-prefixed informative print.
#[macro_export]
macro_rules! dev_info {
    ($dev:expr, $($arg:tt)*) => {
        ostd::info!("{}: {}", $dev.name(), format_args!($($arg)*))
    };
}

/// Linux `dev_warn`: device-prefixed warning print.
#[macro_export]
macro_rules! dev_warn {
    ($dev:expr, $($arg:tt)*) => {
        ostd::warn!("{}: {}", $dev.name(), format_args!($($arg)*))
    };
}

/// Linux `dev_err`: device-prefixed error print.
#[macro_export]
macro_rules! dev_err {
    ($dev:expr, $($arg:tt)*) => {
        ostd::error!("{}: {}", $dev.name(), format_args!($($arg)*))
    };
}

/// Linux `dev_debug`: device-prefixed debug print.
#[macro_export]
macro_rules! dev_debug {
    ($dev:expr, $($arg:tt)*) => {
        ostd::trace!("{}: {}", $dev.name(), format_args!($($arg)*))
    };
}
