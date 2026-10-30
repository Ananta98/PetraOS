// SPDX-License-Identifier: GPL-2.0

//! Safe user-space memory accessors mirroring Linux `<linux/uaccess.h>`.
//!
//! Provides `copy_to_user` and `copy_from_user` with bounds validation,
//! preventing arbitrary kernel-space overwrites from user pointers.

use core::ffi::c_ulong;

use crate::proc::Thread;
use crate::uapi::errno::{Errno, Result};
use crate::utils::userspace::UserPtr;

/// Safely copies `dst.len()` bytes from userspace virtual address `user_addr` into `dst`.
///
/// Guaranteed memory-safe and cannot panic on unmapped addresses or overwrite kernel memory.
pub fn copy_from_user_slice(dst: &mut [u8], user_addr: usize) -> Result<()> {
    if dst.is_empty() {
        return Ok(());
    }
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace.lock().clone();
    let user_ptr = UserPtr::new(user_addr);
    user_ptr.read_bytes(&vmar, dst)
}

/// Safely copies `src.len()` bytes from `src` into userspace virtual address `user_addr`.
///
/// Guaranteed memory-safe and cannot panic on unmapped addresses or overwrite kernel memory.
pub fn copy_to_user_slice(user_addr: usize, src: &[u8]) -> Result<()> {
    if src.is_empty() {
        return Ok(());
    }
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace.lock().clone();
    let user_ptr = UserPtr::new(user_addr);
    user_ptr.write_bytes(&vmar, src)
}

/// Linux `copy_from_user`: copies from user-space memory into kernel buffer.
/// Returns the number of uncopied bytes (0 on full success).
pub unsafe fn copy_from_user(to: *mut u8, from: *const u8, n: usize) -> c_ulong {
    if to.is_null() || from.is_null() || n == 0 {
        return n as c_ulong;
    }

    let dst_slice = unsafe { core::slice::from_raw_parts_mut(to, n) };
    match copy_from_user_slice(dst_slice, from as usize) {
        Ok(()) => 0,
        Err(_) => n as c_ulong,
    }
}

/// Linux `copy_to_user`: copies from kernel buffer into user-space memory.
/// Returns the number of uncopied bytes (0 on full success).
pub unsafe fn copy_to_user(to: *mut u8, from: *const u8, n: usize) -> c_ulong {
    if to.is_null() || from.is_null() || n == 0 {
        return n as c_ulong;
    }

    let src_slice = unsafe { core::slice::from_raw_parts(from, n) };
    match copy_to_user_slice(to as usize, src_slice) {
        Ok(()) => 0,
        Err(_) => n as c_ulong,
    }
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;

    #[ktest]
    fn uaccess_null_and_empty() {
        let mut buf = [0u8; 8];
        assert_eq!(unsafe { copy_from_user(buf.as_mut_ptr(), core::ptr::null(), 8) }, 8);
        assert_eq!(unsafe { copy_from_user(core::ptr::null_mut(), buf.as_ptr(), 8) }, 8);
        assert_eq!(unsafe { copy_from_user(buf.as_mut_ptr(), buf.as_ptr(), 0) }, 0);

        assert_eq!(unsafe { copy_to_user(core::ptr::null_mut(), buf.as_ptr(), 8) }, 8);
        assert_eq!(unsafe { copy_to_user(buf.as_mut_ptr(), core::ptr::null(), 8) }, 8);
        assert_eq!(unsafe { copy_to_user(buf.as_mut_ptr(), buf.as_ptr(), 0) }, 0);

        // Test safe slice accessors with null/invalid address (should return error, not panic)
        assert!(copy_from_user_slice(&mut buf, 0).is_err());
        assert!(copy_to_user_slice(0, &buf).is_err());
        assert!(copy_from_user_slice(&mut [], 0).is_ok());
        assert!(copy_to_user_slice(0, &[]).is_ok());
    }
}

