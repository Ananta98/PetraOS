use crate::drivers::{Device, DeviceType, Driver};

/// A generic trait for all Timer devices.
pub trait Timer: Device + Driver {
    /// Read the current time in nanoseconds since the timer started.
    fn current_time_ns(&self) -> u64;
}

pub mod cmos_rtc;
pub mod tsc;

pub use cmos_rtc::CmosRtc;
pub use tsc::Tsc;
