// SPDX-License-Identifier: GPL-2.0

//! Safe user-space memory accessors mirroring Linux `<linux/uaccess.h>`.
//!
//! Provides `copy_to_user` and `copy_from_user` with bounds validation,
//! preventing arbitrary kernel-space overwrites from user pointers.

use core::ffi::c_ulong;

use crate::proc::Thread;
use crate::utils::userspace::UserPtr;

/// Linux `copy_from_user`: copies from user-space memory into kernel buffer.
/// Returns the number of uncopied bytes (0 on full success).
pub unsafe fn copy_from_user(to: *mut u8, from: *const u8, n: usize) -> c_ulong {
    if to.is_null() || from.is_null() || n == 0 {
        return n as c_ulong;
    }

    if let Some(proc) = Thread::current_proc() {
        let vmar = proc.vmspace.lock().clone();
        let user_ptr = UserPtr::new(from as usize);
        let dst_slice = unsafe { core::slice::from_raw_parts_mut(to, n) };
        if user_ptr.read_bytes(&vmar, dst_slice).is_ok() {
            return 0;
        }
    }

    n as c_ulong
}

/// Linux `copy_to_user`: copies from kernel buffer into user-space memory.
/// Returns the number of uncopied bytes (0 on full success).
pub unsafe fn copy_to_user(to: *mut u8, from: *const u8, n: usize) -> c_ulong {
    if to.is_null() || from.is_null() || n == 0 {
        return n as c_ulong;
    }

    if let Some(proc) = Thread::current_proc() {
        let vmar = proc.vmspace.lock().clone();
        let user_ptr = UserPtr::new(to as usize);
        let src_slice = unsafe { core::slice::from_raw_parts(from, n) };
        if user_ptr.write_bytes(&vmar, src_slice).is_ok() {
            return 0;
        }
    }

    n as c_ulong
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
    }
}

