/// Hardware driver sub-modules.
///
/// The [`Device`], [`DeviceType`], [`Driver`] traits and the global
/// registration functions have moved to [`crate::device`].
/// This module re-exports them for backwards compatibility so that the
/// rest of the codebase (and the driver sub-modules below) can continue
/// to use `crate::drivers::*` paths without change.
pub mod block;
pub mod char;
pub mod gpu;
pub mod irq;
pub mod net;
pub mod pci;
pub mod timer;

pub use block::{BlockDevice, register_block_device};
pub use char::{CharDevice, register_char_device};

// Re-export from the new `device` module so existing `crate::drivers::*`
// paths continue to resolve correctly.
pub use crate::device::{Device, DeviceType, Driver};
pub use crate::device::{register_device, register_driver, unregister_device};
