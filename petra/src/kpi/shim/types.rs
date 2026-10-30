// SPDX-License-Identifier: GPL-2.0

//! Linux C-compatible types and struct definitions.
//!
//! Provides `#[repr(C)]` definitions matching Linux kernel headers:
//! - `<linux/types.h>`
//! - `<linux/device.h>`
//! - `<linux/platform_device.h>`
//! - `<linux/fs.h>`

use core::ffi::{c_char, c_int, c_long, c_uint, c_ulong, c_void};

#[allow(non_camel_case_types)]
pub type dev_t = u32;
#[allow(non_camel_case_types)]
pub type loff_t = i64;
#[allow(non_camel_case_types)]
pub type ssize_t = isize;
#[allow(non_camel_case_types)]
pub type mode_t = u32;


/// Linux `struct device`: C-ABI representation.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LinuxDevice {
    pub name: *const c_char,
    pub parent: *mut LinuxDevice,
    pub driver_data: *mut c_void,
}

unsafe impl Send for LinuxDevice {}
unsafe impl Sync for LinuxDevice {}

impl LinuxDevice {
    pub const fn new() -> Self {
        Self {
            name: core::ptr::null(),
            parent: core::ptr::null_mut(),
            driver_data: core::ptr::null_mut(),
        }
    }
}

/// Linux `struct platform_device`: C-ABI representation.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LinuxPlatformDevice {
    pub name: *const c_char,
    pub id: c_int,
    pub dev: LinuxDevice,
    pub num_resources: u32,
    pub resource: *mut LinuxResource,
}

unsafe impl Send for LinuxPlatformDevice {}
unsafe impl Sync for LinuxPlatformDevice {}

/// Linux `struct resource`: C-ABI representation.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LinuxResource {
    pub start: usize,
    pub end: usize,
    pub name: *const c_char,
    pub flags: c_ulong,
}

unsafe impl Send for LinuxResource {}
unsafe impl Sync for LinuxResource {}

/// Resource flags matching Linux `<linux/ioport.h>`.
pub const IORESOURCE_IO: c_ulong = 0x00000100;
pub const IORESOURCE_MEM: c_ulong = 0x00000200;
pub const IORESOURCE_IRQ: c_ulong = 0x00000400;

/// Linux `struct file`: represents an open file descriptor in Linux driver context.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LinuxFile {
    pub f_pos: loff_t,
    pub private_data: *mut c_void,
    pub flags: c_uint,
    pub mode: mode_t,
}

unsafe impl Send for LinuxFile {}
unsafe impl Sync for LinuxFile {}

impl LinuxFile {
    pub const fn new() -> Self {
        Self {
            f_pos: 0,
            private_data: core::ptr::null_mut(),
            flags: 0,
            mode: 0,
        }
    }
}

/// Linux `struct file_operations`: C function pointer table.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LinuxFileOperations {
    pub open: Option<unsafe extern "C" fn(*mut LinuxFile) -> c_int>,
    pub release: Option<unsafe extern "C" fn(*mut LinuxFile) -> c_int>,
    pub read: Option<unsafe extern "C" fn(*mut LinuxFile, *mut c_char, usize, *mut loff_t) -> ssize_t>,
    pub write: Option<unsafe extern "C" fn(*mut LinuxFile, *const c_char, usize, *mut loff_t) -> ssize_t>,
    pub unlocked_ioctl: Option<unsafe extern "C" fn(*mut LinuxFile, c_uint, c_ulong) -> c_long>,
    pub poll: Option<unsafe extern "C" fn(*mut LinuxFile) -> c_uint>,
}

unsafe impl Send for LinuxFileOperations {}
unsafe impl Sync for LinuxFileOperations {}

/// Linux `struct platform_driver`: driver declaration using C function pointers.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LinuxPlatformDriver {
    pub name: *const c_char,
    pub probe: Option<unsafe extern "C" fn(*mut LinuxPlatformDevice) -> c_int>,
    pub remove: Option<unsafe extern "C" fn(*mut LinuxPlatformDevice) -> c_int>,
}

unsafe impl Send for LinuxPlatformDriver {}
unsafe impl Sync for LinuxPlatformDriver {}

/// Converts a positive Linux integer error code to `Errno`.
pub fn errno_from_code(code: i32) -> crate::uapi::errno::Errno {
    use crate::uapi::errno::Errno;
    match code {
        1 => Errno::EPERM,
        2 => Errno::ENOENT,
        3 => Errno::ESRCH,
        4 => Errno::EINTR,
        5 => Errno::EIO,
        6 => Errno::ENXIO,
        9 => Errno::EBADF,
        11 => Errno::EAGAIN,
        12 => Errno::ENOMEM,
        13 => Errno::EACCES,
        14 => Errno::EFAULT,
        16 => Errno::EBUSY,
        17 => Errno::EEXIST,
        19 => Errno::ENODEV,
        22 => Errno::EINVAL,
        25 => Errno::ENOTTY,
        28 => Errno::ENOSPC,
        _ => Errno::EIO,
    }
}


/// Linux `dev_set_drvdata`: stores driver private data into `dev`.
#[inline]
pub unsafe fn dev_set_drvdata(dev: *mut LinuxDevice, data: *mut c_void) {
    if !dev.is_null() {
        unsafe {
            (*dev).driver_data = data;
        }
    }
}

/// Linux `dev_get_drvdata`: retrieves driver private data from `dev`.
#[inline]
pub unsafe fn dev_get_drvdata(dev: *const LinuxDevice) -> *mut c_void {
    if dev.is_null() {
        core::ptr::null_mut()
    } else {
        unsafe { (*dev).driver_data }
    }
}

