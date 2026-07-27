/// Device and driver management subsystem with Bus-Driven Matching.
///
/// # Module layout
///
/// | File | Responsibility |
/// |------|---------------|
/// | [`bus`]    | [`Bus`] trait and standard bus implementations (PCI, Platform, Virtual) |
/// | [`device`] | [`Device`] trait and [`DeviceType`] enum |
/// | [`driver`] | [`Driver`] trait with Bus-Driven Matching interface |
/// | [`driver_module`] | Kernel module adapter for drivers |
/// | [`manager`] | Global registries, Bus-Driven Matching engine, and `init()` |

pub mod bus;
pub mod device;
pub mod driver;
pub mod driver_module;
pub mod manager;

pub use bus::{Bus, PciBus, PlatformBus, VirtualBus};
pub use device::{Device, DeviceType};
pub use driver::Driver;
pub use manager::{
    register_bus, register_device, register_device_on_bus, register_driver, run_bus_matching,
    unregister_device,
};
