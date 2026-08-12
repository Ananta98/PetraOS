//! Time & Clock Drivers Subsystem

pub mod cmos_rtc;

pub use cmos_rtc::{get_wall_time, read_time, CmosRtc, RtcTime};
