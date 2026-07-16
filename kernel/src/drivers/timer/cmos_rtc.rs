use super::Timer;
use crate::drivers::{Device, DeviceType, Driver};
use ostd::arch::device::io_port::ReadWriteAccess;
use ostd::io::IoPort;

/// CMOS Real-Time Clock (RTC) timer implementation.
pub struct CmosRtc {
    port_index: IoPort<u8, ReadWriteAccess>,
    port_data: IoPort<u8, ReadWriteAccess>,
}

impl CmosRtc {
    /// Create a new CMOS RTC driver instance.
    pub fn new() -> Result<Self, ostd::Error> {
        let port_index = IoPort::acquire_overlapping(0x70)?;
        let port_data = IoPort::acquire_overlapping(0x71)?;
        Ok(Self {
            port_index,
            port_data,
        })
    }

    fn read_register(&self, reg: u8) -> u8 {
        self.port_index.write(reg);
        self.port_data.read()
    }

    fn is_updating(&self) -> bool {
        (self.read_register(0x0A) & 0x80) != 0
    }

    /// Read current date/time from CMOS RTC registers.
    pub fn get_time(&self) -> (u32, u32, u32, u32, u32, u32) {
        while self.is_updating() {
            core::hint::spin_loop();
        }

        let mut seconds = self.read_register(0x00);
        let mut minutes = self.read_register(0x02);
        let mut hours = self.read_register(0x04);
        let mut day = self.read_register(0x07);
        let mut month = self.read_register(0x08);
        let mut year = self.read_register(0x09);

        let register_b = self.read_register(0x0B);

        // Convert BCD to binary if BCD mode is enabled (default on PC RTCs)
        if (register_b & 0x04) == 0 {
            seconds = (seconds & 0x0F) + ((seconds / 16) * 10);
            minutes = (minutes & 0x0F) + ((minutes / 16) * 10);
            hours = ((hours & 0x0F) + (((hours & 0x70) / 16) * 10)) | (hours & 0x80);
            day = (day & 0x0F) + ((day / 16) * 10);
            month = (month & 0x0F) + ((month / 16) * 10);
            year = (year & 0x0F) + ((year / 16) * 10);
        }

        // Convert 12-hour format to 24-hour if needed
        if (register_b & 0x02) == 0 && (hours & 0x80) != 0 {
            hours = ((hours & 0x7F) + 12) % 24;
        }

        let full_year = 2000 + year as u32;

        (
            full_year,
            month as u32,
            day as u32,
            hours as u32,
            minutes as u32,
            seconds as u32,
        )
    }
}

impl Device for CmosRtc {
    fn name(&self) -> &str {
        "cmos-rtc"
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Char
    }
}

impl Driver for CmosRtc {
    fn name(&self) -> &str {
        "cmos-rtc"
    }

    fn probe(&self) -> Result<(), ostd::Error> {
        let reg_a = self.read_register(0x0A);
        if reg_a == 0xFF {
            Err(ostd::Error::InvalidArgs)
        } else {
            Ok(())
        }
    }
}

impl Timer for CmosRtc {
    fn current_time_ns(&self) -> u64 {
        let (year, month, day, hour, min, sec) = self.get_time();
        let seconds = date_to_epoch_seconds(year, month, day, hour, min, sec);
        seconds * 1_000_000_000
    }
}

fn date_to_epoch_seconds(year: u32, month: u32, day: u32, hour: u32, min: u32, sec: u32) -> u64 {
    let days_in_month = [0, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut days = 0u64;

    for y in 1970..year {
        days += if is_leap_year(y) { 366 } else { 365 };
    }

    for m in 1..month {
        days += days_in_month[m as usize] as u64;
        if m == 2 && is_leap_year(year) {
            days += 1;
        }
    }

    days += (day - 1) as u64;

    let hours = days * 24 + hour as u64;
    let minutes = hours * 60 + min as u64;
    let seconds = minutes * 60 + sec as u64;

    seconds
}

fn is_leap_year(year: u32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}
