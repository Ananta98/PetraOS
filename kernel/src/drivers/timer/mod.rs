use crate::drivers::{Device, Driver, DeviceType};
use alloc::sync::Arc;
use ostd::Error;

/// A generic trait for all Timer devices.
pub trait Timer: Device + Driver {
    /// Read the current time in nanoseconds since the timer started.
    fn current_time_ns(&self) -> u64;
}

pub mod tsc;
pub mod cmos_rtc;

pub use tsc::Tsc;
pub use cmos_rtc::CmosRtc;

/// Initialize and register both the TSC and CMOS-RTC timer drivers.
pub fn init() {
    let tsc = Arc::new(Tsc::new());
    let _ = crate::drivers::register_driver(tsc.clone());
    let _ = crate::drivers::register_device(tsc);

    if let Ok(rtc) = CmosRtc::new() {
        let rtc = Arc::new(rtc);
        let _ = crate::drivers::register_driver(rtc.clone());
        let _ = crate::drivers::register_device(rtc);
    }
}
