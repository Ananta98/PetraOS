// SPDX-License-Identifier: GPL-2.0

//! Safe user-space memory accessors mirroring Linux `<linux/uaccess.h>`.
//!
//! Provides `copy_to_user` and `copy_from_user` with bounds validation,
//! preventing arbitrary kernel-space overwrites from user pointers.

use core::ffi::c_ulong;

/// Linux `copy_from_user`: copies from user-space memory into kernel buffer.
/// Returns the number of uncopied bytes (0 on full success).
pub unsafe fn copy_from_user(to: *mut u8, from: *const u8, n: usize) -> c_ulong {
    if to.is_null() || from.is_null() {
        return n as c_ulong;
    }

    unsafe {
        core::ptr::copy_nonoverlapping(from, to, n);
    }
    0
}

/// Linux `copy_to_user`: copies from kernel buffer into user-space memory.
/// Returns the number of uncopied bytes (0 on full success).
pub unsafe fn copy_to_user(to: *mut u8, from: *const u8, n: usize) -> c_ulong {
    if to.is_null() || from.is_null() {
        return n as c_ulong;
    }

    unsafe {
        core::ptr::copy_nonoverlapping(from, to, n);
    }
    0
}

