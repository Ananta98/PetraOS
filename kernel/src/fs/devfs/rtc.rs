//! Real-Time Clock Character Device (/dev/rtc0, /dev/rtc)
//!
//! Provides character device access and POSIX RTC ioctl interface for the CMOS RTC.

use alloc::sync::Arc;
use crate::drivers::time::cmos_rtc::{CmosRtc, get_wall_time};
use crate::fs::vfs::types::{FileOps, InodeOps, Stat, VfsError};
use crate::mm::UserPtr;

/// Linux RTC ioctl commands (x86_64 ABI)
pub const RTC_RD_TIME: u64 = 0x80247009;
pub const RTC_EPOCH_READ: u64 = 0x8008700d;

/// Linux ABI struct rtc_time layout.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct LinuxRtcTime {
    pub tm_sec: i32,
    pub tm_min: i32,
    pub tm_hour: i32,
    pub tm_mday: i32,
    pub tm_mon: i32,   // 0-11
    pub tm_year: i32,  // years since 1900
    pub tm_wday: i32,
    pub tm_yday: i32,
    pub tm_isdst: i32,
}

/// Inode for the `/dev/rtc0` and `/dev/rtc` device.
pub struct RtcInode;

impl InodeOps for RtcInode {
    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        Ok(Arc::new(RtcFileOps))
    }

    fn stat(&self) -> Result<Stat, VfsError> {
        Ok(Stat {
            mode: 0o020644, // S_IFCHR | 0644
            nlink: 1,
            ..Default::default()
        })
    }
}

/// File operations for `/dev/rtc0`.
pub struct RtcFileOps;

impl FileOps for RtcFileOps {
    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, VfsError> {
        let t = CmosRtc::read_hardware_time();
        let s = alloc::format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}\n",
            t.year,
            t.month,
            t.day,
            t.hour,
            t.minute,
            t.second
        );
        let bytes = s.as_bytes();
        if offset >= bytes.len() {
            return Ok(0);
        }
        let copy_len = core::cmp::min(buf.len(), bytes.len() - offset);
        buf[..copy_len].copy_from_slice(&bytes[offset..offset + copy_len]);
        Ok(copy_len)
    }

    fn write(&self, _offset: usize, _buf: &[u8]) -> Result<usize, VfsError> {
        Err(VfsError::PermissionDenied)
    }

    fn ioctl(&self, cmd: u64, arg: usize) -> Result<usize, VfsError> {
        match cmd {
            RTC_RD_TIME => {
                let t = CmosRtc::read_hardware_time();
                let rtc_tm = LinuxRtcTime {
                    tm_sec: t.second as i32,
                    tm_min: t.minute as i32,
                    tm_hour: t.hour as i32,
                    tm_mday: t.day as i32,
                    tm_mon: (t.month.saturating_sub(1)) as i32,
                    tm_year: (t.year.saturating_sub(1900)) as i32,
                    tm_wday: 0,
                    tm_yday: 0,
                    tm_isdst: 0,
                };
                let ptr = UserPtr::<LinuxRtcTime>::from_u64(arg as u64);
                ptr.write(rtc_tm).ok_or(VfsError::InvalidInput)?;
                Ok(0)
            }
            RTC_EPOCH_READ => {
                let (sec, _) = get_wall_time();
                let ptr = UserPtr::<u64>::from_u64(arg as u64);
                ptr.write(sec).ok_or(VfsError::InvalidInput)?;
                Ok(0)
            }
            _ => Err(VfsError::NotSupported),
        }
    }
}
