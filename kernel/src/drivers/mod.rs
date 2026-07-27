/// Hardware driver sub-modules.
pub mod block;
pub mod bus;
pub mod char;
pub mod gpu;
pub mod net;
pub mod timer;

pub use block::{BlockDevice, register_block_device};
pub use bus::pci;
pub use char::{CharDevice, register_char_device};

// Re-export from the `device` module so existing `crate::drivers::*`
// paths continue to resolve correctly.
pub use crate::device::{Device, DeviceType, Driver};
pub use crate::device::{register_device, register_driver, unregister_device};
