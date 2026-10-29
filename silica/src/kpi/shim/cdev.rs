// SPDX-License-Identifier: GPL-2.0

//! Linux `file_operations` and `cdev` C-ABI bridge.
//!
//! Adapts C-style `struct file_operations` into PetraOS's safe `FileOperations` trait.

use alloc::sync::Arc;
use core::ffi::{c_char, c_int, c_uint, c_ulong};

use super::types::{LinuxFile, LinuxFileOperations, loff_t};
use crate::kpi::driver::{cdev_add, CDev, FileOperations};
use crate::uapi::errno::{Errno, Result};


/// Safe Rust wrapper around C-style `LinuxFileOperations`.
pub struct CFileOperationsAdapter {
    fops: LinuxFileOperations,
}

unsafe impl Send for CFileOperationsAdapter {}
unsafe impl Sync for CFileOperationsAdapter {}

impl CFileOperationsAdapter {
    pub fn new(fops: LinuxFileOperations) -> Self {
        Self { fops }
    }
}

impl FileOperations for CFileOperationsAdapter {
    fn open(&self) -> Result<()> {
        if let Some(open_fn) = self.fops.open {
            let mut file = LinuxFile::new();
            let ret = unsafe { open_fn(&mut file) };
            if ret < 0 {
                return Err(super::types::errno_from_code(-ret));
            }
        }
        Ok(())
    }

    fn release(&self) -> Result<()> {
        if let Some(release_fn) = self.fops.release {
            let mut file = LinuxFile::new();
            let ret = unsafe { release_fn(&mut file) };
            if ret < 0 {
                return Err(super::types::errno_from_code(-ret));
            }
        }
        Ok(())
    }

    fn read(&self, buf: &mut [u8], offset: usize) -> Result<usize> {
        let read_fn = self.fops.read.ok_or(Errno::EINVAL)?;
        let mut file = LinuxFile::new();
        let mut ppos: loff_t = offset as loff_t;

        let ret = unsafe {
            read_fn(
                &mut file,
                buf.as_mut_ptr() as *mut c_char,
                buf.len(),
                &mut ppos,
            )
        };

        if ret < 0 {
            Err(super::types::errno_from_code(-ret as i32))
        } else {
            Ok(ret as usize)
        }
    }

    fn write(&self, buf: &[u8], offset: usize) -> Result<usize> {
        let write_fn = self.fops.write.ok_or(Errno::EINVAL)?;
        let mut file = LinuxFile::new();
        let mut ppos: loff_t = offset as loff_t;

        let ret = unsafe {
            write_fn(
                &mut file,
                buf.as_ptr() as *const c_char,
                buf.len(),
                &mut ppos,
            )
        };

        if ret < 0 {
            Err(super::types::errno_from_code(-ret as i32))
        } else {
            Ok(ret as usize)
        }
    }

    fn unlocked_ioctl(&self, cmd: u32, arg: usize) -> Result<usize> {
        let ioctl_fn = self.fops.unlocked_ioctl.ok_or(Errno::ENOTTY)?;
        let mut file = LinuxFile::new();

        let ret = unsafe { ioctl_fn(&mut file, cmd as c_uint, arg as c_ulong) };
        if ret < 0 {
            Err(super::types::errno_from_code(-ret as i32))
        } else {
            Ok(ret as usize)
        }
    }

    fn poll(&self) -> Result<u32> {
        if let Some(poll_fn) = self.fops.poll {
            let mut file = LinuxFile::new();
            let mask = unsafe { poll_fn(&mut file) };
            Ok(mask)
        } else {
            Ok(0)
        }
    }
}

/// Linux `register_chrdev`: C-ABI helper to register a character device using C fops.
pub unsafe extern "C" fn linux_register_chrdev(
    major: c_uint,
    name: *const c_char,
    fops: *const LinuxFileOperations,
) -> c_int {
    if name.is_null() || fops.is_null() {
        return -14; // -EFAULT
    }

    let dev_name = match unsafe { core::ffi::CStr::from_ptr(name) }.to_str() {
        Ok(s) => s,
        Err(_) => return -22, // -EINVAL
    };


    let c_fops = unsafe { *fops };
    let adapter = Arc::new(CFileOperationsAdapter::new(c_fops));
    let cdev = CDev::new(dev_name, major, 0, adapter);

    match cdev_add(cdev) {
        Ok(()) => 0,
        Err(e) => -(e.as_i32()),
    }
}

