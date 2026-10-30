// SPDX-License-Identifier: GPL-2.0

//! Linux `file_operations` and `cdev` C-ABI bridge.
//!
//! Adapts C-style `struct file_operations` into PetraOS's safe `FileOperations` trait.

use alloc::sync::Arc;
use core::ffi::{c_char, c_int, c_uint, c_ulong};
use ostd::sync::Mutex;

use super::types::{LinuxFile, LinuxFileOperations, loff_t};
use crate::kpi::driver::{cdev_add, CDev, FileOperations};
use crate::uapi::errno::{Errno, Result};

/// Safe Rust wrapper around C-style `LinuxFileOperations`.
pub struct CFileOperationsAdapter {
    fops: LinuxFileOperations,
    file: Mutex<LinuxFile>,
}

unsafe impl Send for CFileOperationsAdapter {}
unsafe impl Sync for CFileOperationsAdapter {}

impl CFileOperationsAdapter {
    pub fn new(fops: LinuxFileOperations) -> Self {
        Self {
            fops,
            file: Mutex::new(LinuxFile::new()),
        }
    }
}

impl FileOperations for CFileOperationsAdapter {
    fn open(&self) -> Result<()> {
        let mut file = self.file.lock();
        if let Some(open_fn) = self.fops.open {
            let ret = unsafe { open_fn(&mut *file) };
            if ret < 0 {
                return Err(super::types::errno_from_code(-ret));
            }
        }
        file.f_pos = 0;
        Ok(())
    }

    fn release(&self) -> Result<()> {
        let mut file = self.file.lock();
        if let Some(release_fn) = self.fops.release {
            let ret = unsafe { release_fn(&mut *file) };
            if ret < 0 {
                return Err(super::types::errno_from_code(-ret));
            }
        }
        file.f_pos = 0;
        file.private_data = core::ptr::null_mut();
        Ok(())
    }

    fn read(&self, buf: &mut [u8], offset: usize) -> Result<usize> {
        let read_fn = self.fops.read.ok_or(Errno::EINVAL)?;
        let mut file = self.file.lock();
        let mut ppos: loff_t = if offset > 0 {
            offset as loff_t
        } else {
            file.f_pos
        };

        let ret = unsafe {
            read_fn(
                &mut *file,
                buf.as_mut_ptr() as *mut c_char,
                buf.len(),
                &mut ppos,
            )
        };

        if ret < 0 {
            Err(super::types::errno_from_code(-ret as i32))
        } else {
            file.f_pos = ppos;
            Ok(ret as usize)
        }
    }

    fn write(&self, buf: &[u8], offset: usize) -> Result<usize> {
        let write_fn = self.fops.write.ok_or(Errno::EINVAL)?;
        let mut file = self.file.lock();
        let mut ppos: loff_t = if offset > 0 {
            offset as loff_t
        } else {
            file.f_pos
        };

        let ret = unsafe {
            write_fn(
                &mut *file,
                buf.as_ptr() as *const c_char,
                buf.len(),
                &mut ppos,
            )
        };

        if ret < 0 {
            Err(super::types::errno_from_code(-ret as i32))
        } else {
            file.f_pos = ppos;
            Ok(ret as usize)
        }
    }

    fn unlocked_ioctl(&self, cmd: u32, arg: usize) -> Result<usize> {
        let ioctl_fn = self.fops.unlocked_ioctl.ok_or(Errno::ENOTTY)?;
        let mut file = self.file.lock();

        let ret = unsafe { ioctl_fn(&mut *file, cmd as c_uint, arg as c_ulong) };
        if ret < 0 {
            Err(super::types::errno_from_code(-ret as i32))
        } else {
            Ok(ret as usize)
        }
    }

    fn poll(&self) -> Result<u32> {
        if let Some(poll_fn) = self.fops.poll {
            let mut file = self.file.lock();
            let mask = unsafe { poll_fn(&mut *file) };
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

#[cfg(ktest)]
mod tests {
    use super::*;
    use core::ffi::c_void;
    use ostd::prelude::ktest;
    use crate::kpi::shim::types::ssize_t;

    struct TestState {
        magic: u32,
        buffer: [u8; 16],
        len: usize,
    }

    static mut TEST_DEV: TestState = TestState {
        magic: 0xDEADBEEF,
        buffer: [0u8; 16],
        len: 0,
    };

    unsafe extern "C" fn test_open(file: *mut LinuxFile) -> c_int {
        unsafe {
            (*file).private_data = core::ptr::addr_of_mut!(TEST_DEV) as *mut c_void;
        }
        0
    }

    unsafe extern "C" fn test_write(
        file: *mut LinuxFile,
        buf: *const c_char,
        len: usize,
        ppos: *mut loff_t,
    ) -> ssize_t {
        let dev = unsafe { &mut *((*file).private_data as *mut TestState) };
        assert_eq!(dev.magic, 0xDEADBEEF);
        let to_copy = len.min(16);
        unsafe {
            core::ptr::copy_nonoverlapping(buf as *const u8, dev.buffer.as_mut_ptr(), to_copy);
            *ppos += to_copy as loff_t;
        }
        dev.len = to_copy;
        to_copy as ssize_t
    }

    unsafe extern "C" fn test_read(
        file: *mut LinuxFile,
        buf: *mut c_char,
        len: usize,
        ppos: *mut loff_t,
    ) -> ssize_t {
        let dev = unsafe { &mut *((*file).private_data as *mut TestState) };
        assert_eq!(dev.magic, 0xDEADBEEF);
        let offset = unsafe { *ppos as usize };
        if offset >= dev.len {
            return 0;
        }
        let to_read = (dev.len - offset).min(len);
        unsafe {
            core::ptr::copy_nonoverlapping(dev.buffer.as_ptr().add(offset), buf as *mut u8, to_read);
            *ppos += to_read as loff_t;
        }
        to_read as ssize_t
    }

    unsafe extern "C" fn test_release(file: *mut LinuxFile) -> c_int {
        let dev = unsafe { &mut *((*file).private_data as *mut TestState) };
        assert_eq!(dev.magic, 0xDEADBEEF);
        0
    }

    #[ktest]
    fn cdev_adapter_lifecycle_and_state() {
        let fops = LinuxFileOperations {
            open: Some(test_open),
            release: Some(test_release),
            read: Some(test_read),
            write: Some(test_write),
            unlocked_ioctl: None,
            poll: None,
        };

        let adapter = CFileOperationsAdapter::new(fops);

        // 1. Open should set private_data
        adapter.open().expect("open failed");
        assert_eq!(adapter.file.lock().f_pos, 0);
        assert!(!adapter.file.lock().private_data.is_null());

        // 2. Write should use preserved private_data and update f_pos
        let written = adapter.write(b"hello", 0).expect("write failed");
        assert_eq!(written, 5);
        assert_eq!(adapter.file.lock().f_pos, 5);

        // 3. Read at current f_pos 5 should return 0 (EOF)
        let mut read_buf = [0u8; 5];
        let read_bytes = adapter.read(&mut read_buf, 0).expect("read failed");
        assert_eq!(read_bytes, 0);

        // 4. Release resets private_data and f_pos
        adapter.release().expect("release failed");
        assert_eq!(adapter.file.lock().f_pos, 0);
        assert!(adapter.file.lock().private_data.is_null());
    }
}

