// SPDX-License-Identifier: GPL-2.0

//! Real-Time Clock (RTC) Subsystem.
//!
//! Provides hardware calendar date, time, and boot epoch acquisition.

pub mod cmos;

pub use cmos::{CmosRtc, RtcDateTime, datetime_to_unix_epoch, get_cmos_epoch, get_cmos_time};

use crate::api::errno::Result;

/// Initializes the hardware RTC device.
pub fn init() -> Result<()> {
    cmos::init()
}
