//! CMOS Real-Time Clock (RTC) Driver for x86_64
//!
//! Provides real-time clock reading, BCD decoding, 12/24 hour format normalization,
//! Unix epoch timestamp conversion, and wall-clock time offset calculation.

use crate::arch::ports::Ports;
use crate::device::{Device, DeviceType, Driver, DriverError};
use crate::sync::spinlock::Spinlock;
use alloc::boxed::Box;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicU64, Ordering};

const CMOS_ADDR: u16 = 0x70;
const CMOS_DATA: u16 = 0x71;

// CMOS Register Offsets
const REG_SECONDS: u8 = 0x00;
const REG_MINUTES: u8 = 0x02;
const REG_HOURS: u8 = 0x04;
const REG_DAY_OF_WEEK: u8 = 0x06;
const REG_DAY_OF_MONTH: u8 = 0x07;
const REG_MONTH: u8 = 0x08;
const REG_YEAR: u8 = 0x09;
const REG_STATUS_A: u8 = 0x0A;
const REG_STATUS_B: u8 = 0x0B;
const REG_CENTURY: u8 = 0x32;

/// Global atomic boot epoch timestamp (seconds since Unix epoch 1970-01-01 00:00:00 UTC)
static BOOT_EPOCH_SEC: AtomicU64 = AtomicU64::new(0);
/// HPET elapsed nanoseconds timestamp taken when BOOT_EPOCH_SEC was recorded
static BOOT_HPET_NS: AtomicU64 = AtomicU64::new(0);

/// Date and time structure read from CMOS RTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RtcTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl RtcTime {
    /// Returns true if the given year is a leap year.
    pub fn is_leap_year(year: u16) -> bool {
        (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
    }

    /// Returns the total days before the start of the specified month in a non-leap year.
    pub fn days_before_month(month: u8, year: u16) -> u64 {
        const DAYS: [u64; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
        let idx = (month.saturating_sub(1) as usize).min(11);
        let mut d = DAYS[idx];
        if month > 2 && Self::is_leap_year(year) {
            d += 1;
        }
        d
    }

    /// Converts this `RtcTime` into Unix epoch seconds (seconds since 1970-01-01 00:00:00 UTC).
    pub fn to_epoch(&self) -> u64 {
        let y = self.year as i64;
        if y < 1970 {
            return 0;
        }
        let days_since_1970 =
            (y - 1970) * 365 + (y - 1969) / 4 - (y - 1901) / 100 + (y - 1601) / 400;
        let month_days = Self::days_before_month(self.month, self.year) as i64;
        let day_offset = (self.day.saturating_sub(1)) as i64;
        let total_days = days_since_1970 + month_days + day_offset;

        if total_days < 0 {
            return 0;
        }

        (total_days as u64) * 86400
            + (self.hour as u64) * 3600
            + (self.minute as u64) * 60
            + (self.second as u64)
    }
}

/// Hardware CMOS RTC Driver structure.
pub struct CmosRtc;

impl CmosRtc {
    pub const fn new() -> Self {
        Self
    }

    /// Read a single raw byte from a CMOS register.
    fn read_register(reg: u8) -> u8 {
        // SAFETY: Writing to CMOS address port 0x70 and reading data port 0x71 is safe hardware I/O.
        unsafe {
            // NMI disable bit (0x80) is kept disabled during selection
            Ports::outb(CMOS_ADDR, reg | 0x80);
            Ports::inb(CMOS_DATA)
        }
    }

    /// Returns true if the RTC Update In Progress (UIP) flag is active.
    fn is_update_in_progress() -> bool {
        (Self::read_register(REG_STATUS_A) & 0x80) != 0
    }

    /// Wait until the CMOS RTC update in progress flag clears.
    fn wait_for_update() {
        let mut timeout = 100_000;
        while Self::is_update_in_progress() && timeout > 0 {
            core::hint::spin_loop();
            timeout -= 1;
        }
    }

    /// Convert a Binary-Coded Decimal (BCD) byte to binary.
    #[inline]
    fn bcd_to_bin(val: u8) -> u8 {
        ((val >> 4) * 10) + (val & 0x0F)
    }

    /// Read current time and date from CMOS hardware.
    pub fn read_hardware_time() -> RtcTime {
        let mut last_sec;
        let mut last_min;
        let mut last_hour;
        let mut last_day;
        let mut last_month;
        let mut last_year;
        let mut last_century;

        let mut sec;
        let mut min;
        let mut hour;
        let mut day;
        let mut month;
        let mut year;
        let mut century;

        // Loop until two consecutive reads yield identical results to avoid torn reads across RTC second rollover.
        loop {
            Self::wait_for_update();
            sec = Self::read_register(REG_SECONDS);
            min = Self::read_register(REG_MINUTES);
            hour = Self::read_register(REG_HOURS);
            day = Self::read_register(REG_DAY_OF_MONTH);
            month = Self::read_register(REG_MONTH);
            year = Self::read_register(REG_YEAR);
            century = Self::read_register(REG_CENTURY);

            Self::wait_for_update();
            last_sec = Self::read_register(REG_SECONDS);
            last_min = Self::read_register(REG_MINUTES);
            last_hour = Self::read_register(REG_HOURS);
            last_day = Self::read_register(REG_DAY_OF_MONTH);
            last_month = Self::read_register(REG_MONTH);
            last_year = Self::read_register(REG_YEAR);
            last_century = Self::read_register(REG_CENTURY);

            if sec == last_sec
                && min == last_min
                && hour == last_hour
                && day == last_day
                && month == last_month
                && year == last_year
                && century == last_century
            {
                break;
            }
        }

        let status_b = Self::read_register(REG_STATUS_B);
        let is_binary = (status_b & 0x04) != 0;
        let is_24h = (status_b & 0x02) != 0;

        if !is_binary {
            sec = Self::bcd_to_bin(sec);
            min = Self::bcd_to_bin(min);
            hour = Self::bcd_to_bin(hour & 0x7F) | (hour & 0x80);
            day = Self::bcd_to_bin(day);
            month = Self::bcd_to_bin(month);
            year = Self::bcd_to_bin(year);
            if century != 0 {
                century = Self::bcd_to_bin(century);
            }
        }

        if !is_24h && (hour & 0x80) != 0 {
            hour = ((hour & 0x7F) + 12) % 24;
        }

        let full_year = if century > 0 {
            (century as u16) * 100 + (year as u16)
        } else if year < 70 {
            2000 + (year as u16)
        } else {
            1900 + (year as u16)
        };

        RtcTime {
            year: full_year,
            month,
            day,
            hour,
            minute: min,
            second: sec,
        }
    }
}

pub static CMOS_RTC: Spinlock<CmosRtc> = Spinlock::new(CmosRtc::new());

pub struct CmosRtcDeviceRef;

impl Device for CmosRtcDeviceRef {
    fn dev_type(&self) -> DeviceType {
        DeviceType::Char
    }

    fn name(&self) -> &'static str {
        "CMOS Real-Time Clock"
    }

    fn init(&mut self) -> Result<(), DriverError> {
        init_boot_time();
        Ok(())
    }
}

/// Initialize kernel boot time from CMOS RTC.
pub fn init_boot_time() {
    let rtc_time = CmosRtc::read_hardware_time();
    let epoch_sec = rtc_time.to_epoch();
    let hpet_ns = crate::arch::timer::hpet::elapsed_ns();

    BOOT_EPOCH_SEC.store(epoch_sec, Ordering::Relaxed);
    BOOT_HPET_NS.store(hpet_ns, Ordering::Relaxed);

    log::info!(
        "[CMOS RTC] Initialized real-time clock: {:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC (Epoch: {})",
        rtc_time.year,
        rtc_time.month,
        rtc_time.day,
        rtc_time.hour,
        rtc_time.minute,
        rtc_time.second,
        epoch_sec
    );
}

/// Read current CMOS hardware date and time.
pub fn read_time() -> RtcTime {
    CmosRtc::read_hardware_time()
}

/// Returns current wall clock time as `(seconds, microseconds)` since Unix epoch.
pub fn get_wall_time() -> (u64, u64) {
    let boot_sec = BOOT_EPOCH_SEC.load(Ordering::Relaxed);
    let boot_ns = BOOT_HPET_NS.load(Ordering::Relaxed);

    if boot_sec == 0 {
        // If boot time hasn't been cached yet, attempt to read now.
        let rtc_time = CmosRtc::read_hardware_time();
        let sec = rtc_time.to_epoch();
        return (sec, 0);
    }

    let current_hpet_ns = crate::arch::timer::hpet::elapsed_ns();
    let elapsed_ns = current_hpet_ns.saturating_sub(boot_ns);

    let total_sec = boot_sec + (elapsed_ns / 1_000_000_000);
    let usec = (elapsed_ns % 1_000_000_000) / 1_000;

    (total_sec, usec)
}

#[derive(Default)]
pub struct CmosRtcDriver;

impl Driver for CmosRtcDriver {
    fn name(&self) -> &'static str {
        "cmos_rtc"
    }

    fn bus_name(&self) -> &'static str {
        "platform"
    }

    fn description(&self) -> &'static str {
        "CMOS Real-Time Clock Driver"
    }

    fn probe(&self) -> Result<(), DriverError> {
        init_boot_time();
        let device_ref: Arc<Spinlock<Box<dyn Device>>> =
            Arc::new(Spinlock::new(Box::new(CmosRtcDeviceRef)));
        crate::device::DEVICE_MANAGER.write().register(device_ref);
        log::info!("[CMOS RTC Module] Registered CMOS Real-Time Clock to DEVICE_MANAGER");
        Ok(())
    }
}

crate::MODULE_LICENSE!("BSD-2-Clause");
crate::MODULE_AUTHOR!("Ananta98");
crate::MODULE_DESCRIPTION!("CMOS Real-Time Clock Driver");
crate::MODULE_VERSION!("1.0.0");
crate::module_driver!(
    CMOS_RTC_INITCALL,
    cmos_rtc_driver_init,
    "cmos_rtc",
    CmosRtcDriver
);
