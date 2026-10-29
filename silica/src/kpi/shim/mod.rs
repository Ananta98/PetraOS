// SPDX-License-Identifier: GPL-2.0

//! Linux Driver Compatibility SHIM layer (FreeBSD-style).
//!
//! Provides the C-ABI compatibility bridge, struct representations, and calling
//! convention adapters for running ported Linux drivers on top of `silica::kpi`.

pub mod cdev;
pub mod irq;
pub mod platform;
pub mod types;
pub mod uaccess;


pub use cdev::{CFileOperationsAdapter, linux_register_chrdev};
pub use irq::{linux_free_irq, linux_request_irq, LinuxIrqHandler};
pub use platform::{linux_platform_driver_register, CPlatformDriverAdapter};
pub use types::*;
pub use uaccess::{copy_from_user, copy_to_user};
