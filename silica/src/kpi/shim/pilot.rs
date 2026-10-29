// SPDX-License-Identifier: GPL-2.0

//! Pilot Test Driver exercising the C-compatible Linux Driver SHIM.
//!
//! Validates:
//! 1. Platform driver registration via C function pointers (`probe`, `remove`).
//! 2. Platform device probing and `dev_set_drvdata` / `dev_get_drvdata` pointer handling.
//! 3. Character device node creation (`/dev/kpi_pilot`) with Linux `struct file_operations`.
//! 4. User-buffer data transfer via `copy_to_user` and `copy_from_user`.

use alloc::sync::Arc;
use core::ffi::{c_char, c_int, c_void};
use spin::Mutex;

use super::types::{
    dev_get_drvdata, dev_set_drvdata, LinuxFile, LinuxFileOperations, LinuxPlatformDevice,
    LinuxPlatformDriver, loff_t, ssize_t,
};
use super::uaccess::{copy_from_user, copy_to_user};
use crate::kpi::device::{platform_device_register, PlatformDevice, Resource};
use crate::uapi::errno::Result;

/// Mock hardware state managed by the pilot driver.
struct PilotDeviceState {
    buffer: Mutex<[u8; 64]>,
    len: Mutex<usize>,
}

static PILOT_STATE: PilotDeviceState = PilotDeviceState {
    buffer: Mutex::new([0u8; 64]),
    len: Mutex::new(0),
};

static DRIVER_NAME: &[u8] = b"kpi_pilot_drv\0";
static DEV_NAME: &[u8] = b"kpi_pilot\0";

unsafe extern "C" fn pilot_open(_file: *mut LinuxFile) -> c_int {
    ostd::info!("kpi_pilot: C-SHIM file opened");
    0
}

unsafe extern "C" fn pilot_release(_file: *mut LinuxFile) -> c_int {
    ostd::info!("kpi_pilot: C-SHIM file closed");
    0
}

unsafe extern "C" fn pilot_read(
    _file: *mut LinuxFile,
    buf: *mut c_char,
    count: usize,
    _ppos: *mut loff_t,
) -> ssize_t {
    let len_guard = PILOT_STATE.len.lock();
    let bytes_to_read = (*len_guard).min(count);
    if bytes_to_read == 0 {
        return 0;
    }

    let buf_guard = PILOT_STATE.buffer.lock();
    let uncopied = unsafe { copy_to_user(buf as *mut u8, buf_guard.as_ptr(), bytes_to_read) };
    let bytes_read = bytes_to_read - uncopied as usize;
    bytes_read as ssize_t
}

unsafe extern "C" fn pilot_write(
    _file: *mut LinuxFile,
    buf: *const c_char,
    count: usize,
    _ppos: *mut loff_t,
) -> ssize_t {
    let mut buf_guard = PILOT_STATE.buffer.lock();
    let bytes_to_write = count.min(buf_guard.len());

    let uncopied = unsafe { copy_from_user(buf_guard.as_mut_ptr(), buf as *const u8, bytes_to_write) };
    let bytes_written = bytes_to_write - uncopied as usize;
    *PILOT_STATE.len.lock() = bytes_written;

    bytes_written as ssize_t
}

static PILOT_FOPS: LinuxFileOperations = LinuxFileOperations {
    open: Some(pilot_open),
    release: Some(pilot_release),
    read: Some(pilot_read),
    write: Some(pilot_write),
    unlocked_ioctl: None,
    poll: None,
};

unsafe extern "C" fn pilot_probe(pdev: *mut LinuxPlatformDevice) -> c_int {
    if pdev.is_null() {
        return -14; // -EFAULT
    }

    ostd::info!("kpi_pilot: C-SHIM platform probe invoked successfully!");

    // Set private driver data via C accessor
    let state_ptr = &PILOT_STATE as *const PilotDeviceState as *mut c_void;
    unsafe {
        dev_set_drvdata(&mut (*pdev).dev, state_ptr);

        // Register character device /dev/kpi_pilot
        let ret = super::cdev::linux_register_chrdev(
            245,
            DEV_NAME.as_ptr() as *const c_char,
            &PILOT_FOPS,
        );
        if ret != 0 {
            ostd::error!("kpi_pilot: Failed to register chrdev, err: {}", ret);
            return ret;
        }
    }

    ostd::info!("kpi_pilot: Registered /dev/kpi_pilot via C-SHIM file_operations");
    0
}

unsafe extern "C" fn pilot_remove(pdev: *mut LinuxPlatformDevice) -> c_int {
    if pdev.is_null() {
        return -14;
    }

    let _data = unsafe { dev_get_drvdata(&(*pdev).dev) };
    ostd::info!("kpi_pilot: C-SHIM platform remove invoked");
    0
}

static PILOT_DRIVER: LinuxPlatformDriver = LinuxPlatformDriver {
    name: DRIVER_NAME.as_ptr() as *const c_char,
    probe: Some(pilot_probe),
    remove: Some(pilot_remove),
};


/// Initializes the pilot platform device and registers the C-SHIM platform driver.
pub fn init() -> Result<()> {
    // 1. Register C platform driver
    unsafe {
        let ret = super::platform::linux_platform_driver_register(&PILOT_DRIVER);
        if ret != 0 {
            ostd::error!("kpi_pilot: Failed to register C platform driver");
            crate::return_errno!(EIO, "failed to register pilot driver");
        }
    }

    // 2. Register matching platform device to trigger probe
    let resources = alloc::vec![
        Resource::mem(0x1000_0000, 0x1000, "pilot_mmio"),
        Resource::irq(15, "pilot_irq"),
    ];
    let pdev = Arc::new(PlatformDevice::new("kpi_pilot_drv", -1, resources));
    platform_device_register(pdev)?;

    Ok(())
}
