/// Device and driver management subsystem.
///
/// # Module layout
///
/// | File | Responsibility |
/// |------|---------------|
/// | [`device`]  | [`Device`] trait and [`DeviceType`] enum |
/// | [`driver`]  | [`Driver`] trait |
/// | [`manager`] | Global registries, registration helpers, and `init()` |
///
/// # Public surface
///
/// Most kernel code should use the top-level re-exports:
///
/// ```rust,ignore
/// use crate::device::{Device, DeviceType, Driver};
/// use crate::device::{register_device, register_driver, unregister_device};
/// ```
pub mod device;
pub mod driver;
pub mod manager;

pub use device::{Device, DeviceType};
pub use driver::Driver;
pub use manager::{init, register_device, register_driver, unregister_device};
