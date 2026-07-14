use crate::drivers::{Device, DeviceType, register_device};
use crate::fs::vfs::{DirEntry, FileOps, FileType, InodeOps, Metadata, SeekFrom};
use alloc::boxed::Box;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use ostd::arch::device::io_port::ReadWriteAccess;
use ostd::io::IoPort;
use spin::Once;

/// CMOS address select port.
const CMOS_ADDR_PORT: u16 = 0x70;
/// CMOS data port.
const CMOS_DATA_PORT: u16 = 0x71;

/// Represents a calendar date and time read from the RTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RtcTime {
    pub second: u8,
    pub minute: u8,
    pub hour: u8,
    pub day: u8,
    pub month: u8,
    pub year: u32,
}

/// Helper function to check if the RTC update is in progress.
fn is_update_in_progress(addr: &IoPort<u8, ReadWriteAccess>, data: &IoPort<u8, ReadWriteAccess>) -> bool {
    addr.write(0x0A);
    (data.read() & 0x80) != 0
}

/// Helper function to read a specific RTC register.
fn read_register(addr: &IoPort<u8, ReadWriteAccess>, data: &IoPort<u8, ReadWriteAccess>, reg: u8) -> u8 {
    addr.write(reg);
    data.read()
}

/// Read the current time from the hardware Real-Time Clock.
///
/// BCD values and 12-hour/24-hour formats are standardized to binary 24-hour formats.
pub fn read_time() -> Result<RtcTime, ostd::Error> {
    let addr = IoPort::<u8, ReadWriteAccess>::acquire(CMOS_ADDR_PORT)?;
    let data = IoPort::<u8, ReadWriteAccess>::acquire(CMOS_DATA_PORT)?;

    // Wait until there is no update in progress.
    while is_update_in_progress(&addr, &data) {
        core::hint::spin_loop();
    }

    let mut second = read_register(&addr, &data, 0x00);
    let mut minute = read_register(&addr, &data, 0x02);
    let mut hour = read_register(&addr, &data, 0x04);
    let mut day = read_register(&addr, &data, 0x07);
    let mut month = read_register(&addr, &data, 0x08);
    let mut year = read_register(&addr, &data, 0x09) as u32;

    // Re-read until values are stable to avoid rollover discrepancy.
    loop {
        if is_update_in_progress(&addr, &data) {
            continue;
        }
        let sec2 = read_register(&addr, &data, 0x00);
        let min2 = read_register(&addr, &data, 0x02);
        let hr2 = read_register(&addr, &data, 0x04);
        let day2 = read_register(&addr, &data, 0x07);
        let mon2 = read_register(&addr, &data, 0x08);
        let yr2 = read_register(&addr, &data, 0x09) as u32;

        if second == sec2 && minute == min2 && hour == hr2 && day == day2 && month == mon2 && year == yr2 {
            break;
        }
        second = sec2;
        minute = min2;
        hour = hr2;
        day = day2;
        month = mon2;
        year = yr2;
    }

    let register_b = read_register(&addr, &data, 0x0B);

    // Convert BCD to binary if bit 2 of Register B is clear.
    if (register_b & 0x04) == 0 {
        second = (second & 0x0F) + ((second / 16) * 10);
        minute = (minute & 0x0F) + ((minute / 16) * 10);
        hour = ((hour & 0x0F) + (((hour & 0x70) / 16) * 10)) | (hour & 0x80);
        day = (day & 0x0F) + ((day / 16) * 10);
        month = (month & 0x0F) + ((month / 16) * 10);
        year = (year & 0x0F) + ((year / 16) * 10);
    }

    // Convert 12-hour format to 24-hour if bit 1 of Register B is clear.
    if (register_b & 0x02) == 0 && (hour & 0x80) != 0 {
        hour = ((hour & 0x7F) + 12) % 24;
    }

    year += 2000;

    Ok(RtcTime {
        second,
        minute,
        hour,
        day,
        month,
        year,
    })
}

// ---------------------------------------------------------------------------
// VFS / Device Glue
// ---------------------------------------------------------------------------

struct RtcDevice {
    name: String,
}

impl Device for RtcDevice {
    fn name(&self) -> &str {
        &self.name
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Char
    }

    fn inode_ops(&self) -> Option<Arc<dyn InodeOps>> {
        Some(Arc::new(RtcInode))
    }
}

struct RtcInode;

impl InodeOps for RtcInode {
    fn lookup(&self, _name: &str) -> Result<Arc<dyn InodeOps>, ostd::Error> {
        Err(ostd::Error::InvalidArgs)
    }

    fn create(&self, _name: &str, _mode: u32) -> Result<Arc<dyn InodeOps>, ostd::Error> {
        Err(ostd::Error::InvalidArgs)
    }

    fn mkdir(&self, _name: &str, _mode: u32) -> Result<Arc<dyn InodeOps>, ostd::Error> {
        Err(ostd::Error::InvalidArgs)
    }

    fn symlink(&self, _name: &str, _target: &str) -> Result<Arc<dyn InodeOps>, ostd::Error> {
        Err(ostd::Error::InvalidArgs)
    }

    fn metadata(&self) -> Result<Metadata, ostd::Error> {
        Ok(Metadata {
            size: 0,
            file_type: FileType::CharDevice,
            mode: 0o660,
            inode_num: 0,
            nlink: 1,
        })
    }

    fn read_link(&self) -> Result<String, ostd::Error> {
        Err(ostd::Error::InvalidArgs)
    }

    fn open(&self, _flags: u32) -> Result<Box<dyn FileOps>, ostd::Error> {
        Ok(Box::new(RtcFile))
    }

    fn as_any(&self) -> &dyn core::any::Any {
        self
    }

    fn unlink(&self, _name: &str) -> Result<(), ostd::Error> {
        Err(ostd::Error::InvalidArgs)
    }

    fn rename(
        &self,
        _old_name: &str,
        _new_parent: &Arc<dyn InodeOps>,
        _new_name: &str,
    ) -> Result<(), ostd::Error> {
        Err(ostd::Error::InvalidArgs)
    }
}

struct RtcFile;

impl FileOps for RtcFile {
    fn read(&mut self, buf: &mut [u8], offset: &mut usize) -> Result<usize, ostd::Error> {
        if *offset > 0 {
            return Ok(0);
        }
        let time = read_time()?;
        let time_str = alloc::format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}\n",
            time.year, time.month, time.day, time.hour, time.minute, time.second
        );
        let bytes = time_str.as_bytes();
        let len = core::cmp::min(buf.len(), bytes.len());
        buf[..len].copy_from_slice(&bytes[..len]);
        *offset += len;
        Ok(len)
    }

    fn write(&mut self, _buf: &[u8], _offset: &mut usize) -> Result<usize, ostd::Error> {
        Err(ostd::Error::InvalidArgs)
    }

    fn seek(&mut self, _pos: SeekFrom, offset: &mut usize) -> Result<usize, ostd::Error> {
        Ok(*offset)
    }

    fn readdir(&mut self) -> Result<Vec<DirEntry>, ostd::Error> {
        Err(ostd::Error::InvalidArgs)
    }
}

static RTC_DEVICE: Once<Arc<RtcDevice>> = Once::new();

/// Initialize and register the Real-Time Clock device.
pub fn init() {
    let rtc = Arc::new(RtcDevice {
        name: String::from("rtc"),
    });
    let _ = register_device(rtc.clone());
    RTC_DEVICE.call_once(|| rtc);
}

// ---------------------------------------------------------------------------
// Unit Tests Block
// ---------------------------------------------------------------------------

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;

    #[ktest]
    fn test_rtc_read_time() {
        // Verify we can read the time successfully without error.
        let time = read_time();
        assert!(time.is_ok());
        let t = time.unwrap();
        // Years should be reasonable (at least 2026).
        assert!(t.year >= 2026);
        assert!(t.month >= 1 && t.month <= 12);
        assert!(t.day >= 1 && t.day <= 31);
        assert!(t.hour < 24);
        assert!(t.minute < 60);
        assert!(t.second < 60);
    }
}
