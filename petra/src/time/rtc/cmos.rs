// SPDX-License-Identifier: GPL-2.0

//! CMOS Real-Time Clock (RTC) Driver for x86_64 architectures.
//!
//! Controls the Motorola MC146818-compatible Real Time Clock chip via
//! legacy I/O ports 0x70 (Index / NMI) and 0x71 (Data).

use ostd::{
    arch::device::io_port::ReadWriteAccess,
    io::IoPort,
    sync::{LocalIrqDisabled, SpinLock},
};
use spin::Once;

use crate::uapi::errno::{Errno, Result};

// ----------------------------------------------------------------------------
// I/O Port Addresses
// ----------------------------------------------------------------------------

pub const CMOS_ADDR_PORT: u16 = 0x70;
pub const CMOS_DATA_PORT: u16 = 0x71;

// ----------------------------------------------------------------------------
// RTC Registers
// ----------------------------------------------------------------------------

pub const RTC_REG_SECONDS: u8 = 0x00;
pub const RTC_REG_MINUTES: u8 = 0x02;
pub const RTC_REG_HOURS: u8 = 0x04;
pub const RTC_REG_DAY_OF_WEEK: u8 = 0x06;
pub const RTC_REG_DAY_OF_MONTH: u8 = 0x07;
pub const RTC_REG_MONTH: u8 = 0x08;
pub const RTC_REG_YEAR: u8 = 0x09;
pub const RTC_REG_STATUS_A: u8 = 0x0A;
pub const RTC_REG_STATUS_B: u8 = 0x0B;
pub const RTC_REG_CENTURY: u8 = 0x32;

// Status Register A flags
pub const STATUS_A_UIP: u8 = 0x80; // Update In Progress

// Status Register B flags
pub const STATUS_B_24HR: u8 = 0x02; // 24-hour format (1 = 24hr, 0 = 12hr)
pub const STATUS_B_BINARY: u8 = 0x04; // Binary mode (1 = binary, 0 = BCD)

// ----------------------------------------------------------------------------
// Date & Time Structure
// ----------------------------------------------------------------------------

/// Represents a calendar date and wall-clock time read from the RTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RtcDateTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl RtcDateTime {
    /// Converts this date and time into seconds since UNIX epoch (1970-01-01 00:00:00 UTC).
    pub fn to_unix_epoch_secs(&self) -> u64 {
        datetime_to_unix_epoch(
            self.year,
            self.month,
            self.day,
            self.hour,
            self.minute,
            self.second,
        )
    }
}

/// Converts a Gregorian calendar date and time to seconds since UNIX epoch.
pub fn datetime_to_unix_epoch(
    year: u16,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
) -> u64 {
    // Cumulative days before each month in a standard (non-leap) year (1-indexed)
    const DAYS_BEFORE_MONTH: [u32; 13] = [0, 0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];

    #[inline]
    fn is_leap_year(y: u16) -> bool {
        (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0)
    }

    if year < 1970 || month == 0 || month > 12 || day == 0 || day > 31 {
        return 0;
    }

    let mut days: u64 = 0;
    for y in 1970..year {
        days += if is_leap_year(y) { 366 } else { 365 };
    }

    let m = month as usize;
    days += DAYS_BEFORE_MONTH[m] as u64;
    if m > 2 && is_leap_year(year) {
        days += 1;
    }

    days += (day as u64).saturating_sub(1);

    days * 86400 + (hour as u64) * 3600 + (minute as u64) * 60 + (second as u64)
}

// ----------------------------------------------------------------------------
// CMOS Controller Abstraction
// ----------------------------------------------------------------------------

/// Safe hardware driver for the CMOS Real Time Clock.
pub struct CmosRtc {
    addr_port: IoPort<u8, ReadWriteAccess>,
    data_port: IoPort<u8, ReadWriteAccess>,
}

impl CmosRtc {
    /// Acquires the CMOS I/O ports.
    pub fn new() -> Result<Self> {
        let addr_port = IoPort::<u8, ReadWriteAccess>::acquire_overlapping(CMOS_ADDR_PORT)
            .map_err(|_| Errno::EBUSY)?;
        let data_port = IoPort::<u8, ReadWriteAccess>::acquire_overlapping(CMOS_DATA_PORT)
            .map_err(|_| Errno::EBUSY)?;

        Ok(Self {
            addr_port,
            data_port,
        })
    }

    /// Reads a CMOS register by index while preserving NMI enable state.
    pub fn read_reg(&self, reg: u8) -> u8 {
        // Bit 7 is NMI disable; keeping it 0 keeps NMIs enabled.
        self.addr_port.write(reg & 0x7F);
        self.data_port.read()
    }

    /// Writes a value to a CMOS register by index.
    pub fn write_reg(&self, reg: u8, val: u8) {
        self.addr_port.write(reg & 0x7F);
        self.data_port.write(val);
    }

    /// Checks if an update cycle is currently in progress.
    pub fn is_update_in_progress(&self) -> bool {
        (self.read_reg(RTC_REG_STATUS_A) & STATUS_A_UIP) != 0
    }

    /// Reads raw time registers into an uncorrected `RtcDateTime`.
    fn read_raw_snapshot(&self) -> (RtcDateTime, u8, u8) {
        let second = self.read_reg(RTC_REG_SECONDS);
        let minute = self.read_reg(RTC_REG_MINUTES);
        let hour = self.read_reg(RTC_REG_HOURS);
        let day = self.read_reg(RTC_REG_DAY_OF_MONTH);
        let month = self.read_reg(RTC_REG_MONTH);
        let year = self.read_reg(RTC_REG_YEAR);
        let century = self.read_reg(RTC_REG_CENTURY);

        let dt = RtcDateTime {
            year: year as u16,
            month,
            day,
            hour,
            minute,
            second,
        };

        (dt, century, self.read_reg(RTC_REG_STATUS_B))
    }

    /// Reads the current calendar date and time reliably.
    ///
    /// Waits until the UIP (Update In Progress) flag is clear and performs
    /// consecutive reads to guard against mid-read counter rollovers.
    pub fn read_datetime(&self) -> Result<RtcDateTime> {
        // Wait until UIP is clear (with spin limit)
        for _ in 0..10_000 {
            if !self.is_update_in_progress() {
                break;
            }
            core::hint::spin_loop();
        }

        // Perform double-read to guarantee consistency across tick boundaries
        let mut snapshot1 = self.read_raw_snapshot();
        let mut snapshot2 = self.read_raw_snapshot();

        let mut retries = 0;
        while snapshot1 != snapshot2 && retries < 10 {
            snapshot1 = snapshot2;
            snapshot2 = self.read_raw_snapshot();
            retries += 1;
        }

        let (mut dt, raw_century, status_b) = snapshot2;

        let is_binary = (status_b & STATUS_B_BINARY) != 0;
        let is_24hr = (status_b & STATUS_B_24HR) != 0;

        // Convert BCD to binary if required
        #[inline]
        fn bcd_to_bin(val: u8) -> u8 {
            (val & 0x0F) + ((val >> 4) * 10)
        }

        if !is_binary {
            dt.second = bcd_to_bin(dt.second);
            dt.minute = bcd_to_bin(dt.minute);
            dt.hour = bcd_to_bin(dt.hour & 0x7F) | (dt.hour & 0x80);
            dt.day = bcd_to_bin(dt.day);
            dt.month = bcd_to_bin(dt.month);
            dt.year = bcd_to_bin(dt.year as u8) as u16;
        }

        // Convert 12-hour format to 24-hour format if required
        if !is_24hr {
            let is_pm = (dt.hour & 0x80) != 0;
            let mut h = dt.hour & 0x7F;
            if is_pm && h < 12 {
                h += 12;
            } else if !is_pm && h == 12 {
                h = 0;
            }
            dt.hour = h;
        }

        // Resolve 4-digit year: check century register or infer modern century
        let century_val = if !is_binary && raw_century != 0 {
            bcd_to_bin(raw_century) as u16
        } else {
            raw_century as u16
        };

        if (19..=21).contains(&century_val) {
            dt.year += century_val * 100;
        } else if dt.year >= 70 {
            dt.year += 1900;
        } else {
            dt.year += 2000;
        }

        Ok(dt)
    }

    /// Reads the current wall-clock epoch timestamp directly from RTC.
    pub fn read_epoch_secs(&self) -> Result<u64> {
        let dt = self.read_datetime()?;
        Ok(dt.to_unix_epoch_secs())
    }
}

// ----------------------------------------------------------------------------
// Global Instance & Public Helper API
// ----------------------------------------------------------------------------

static CMOS_RTC: Once<SpinLock<CmosRtc, LocalIrqDisabled>> = Once::new();

/// Initializes the CMOS RTC driver.
pub fn init() -> Result<()> {
    let rtc = CmosRtc::new()?;
    if let Ok(dt) = rtc.read_datetime() {
        ostd::info!(
            "RTC: CMOS initialized; system time is {:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC (epoch: {}s)",
            dt.year,
            dt.month,
            dt.day,
            dt.hour,
            dt.minute,
            dt.second,
            dt.to_unix_epoch_secs()
        );
    }
    CMOS_RTC.call_once(|| SpinLock::new(rtc));
    Ok(())
}

/// Reads the current date and time from the global CMOS RTC.
pub fn get_cmos_time() -> Result<RtcDateTime> {
    let rtc = CMOS_RTC.get().ok_or(Errno::ENODEV)?;
    rtc.lock().read_datetime()
}

/// Reads the current UNIX epoch seconds from the global CMOS RTC.
pub fn get_cmos_epoch() -> Result<u64> {
    let rtc = CMOS_RTC.get().ok_or(Errno::ENODEV)?;
    rtc.lock().read_epoch_secs()
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;

    #[ktest]
    fn datetime_to_unix_epoch_baseline() {
        // 1970-01-01 00:00:00 UTC = 0
        assert_eq!(datetime_to_unix_epoch(1970, 1, 1, 0, 0, 0), 0);

        // 1970-01-01 01:00:00 UTC = 3600
        assert_eq!(datetime_to_unix_epoch(1970, 1, 1, 1, 0, 0), 3600);

        // 2000-01-01 00:00:00 UTC = 946684800
        assert_eq!(datetime_to_unix_epoch(2000, 1, 1, 0, 0, 0), 946684800);

        // 2026-04-07 12:04:00 UTC = 1775563440
        assert_eq!(datetime_to_unix_epoch(2026, 4, 7, 12, 4, 0), 1775563440);
    }
}
