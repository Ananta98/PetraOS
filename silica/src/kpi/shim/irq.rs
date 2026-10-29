// SPDX-License-Identifier: GPL-2.0

//! Linux `request_irq` C-ABI bridge.
//!
//! Exposes Linux-style `request_irq` taking function pointers and `void *dev_id`.

use core::ffi::{c_char, c_int, c_uint, c_ulong, c_void};

use crate::kpi::hw::{free_irq, request_irq, IrqFlags, IrqReturn};

/// Linux C interrupt handler signature: `irqreturn_t (*handler)(int, void *)`.
pub type LinuxIrqHandler = unsafe extern "C" fn(c_int, *mut c_void) -> c_int;

/// Linux `request_irq` C-ABI wrapper.
pub unsafe extern "C" fn linux_request_irq(
    irq: c_uint,
    handler: LinuxIrqHandler,
    flags: c_ulong,
    name: *const c_char,
    dev_id: *mut c_void,
) -> c_int {
    if name.is_null() {
        return -14; // -EFAULT
    }

    let dev_name = match unsafe { core::ffi::CStr::from_ptr(name) }.to_str() {
        Ok(s) => s,
        Err(_) => return -22, // -EINVAL
    };

    let dev_id_val = dev_id as usize;
    let rust_flags = if flags & 0x00000080 != 0 {
        IrqFlags::SHARED
    } else {
        IrqFlags::NONE
    };

    let res = request_irq(
        irq as usize,
        move || {
            let ret = unsafe { handler(irq as c_int, dev_id_val as *mut c_void) };
            if ret == 1 {
                IrqReturn::Handled
            } else if ret == 2 {
                IrqReturn::WakeThread
            } else {
                IrqReturn::None
            }
        },
        rust_flags,
        dev_name,
    );

    match res {
        Ok(()) => 0,
        Err(e) => -(e.as_i32()),
    }
}

/// Linux `free_irq` C-ABI wrapper.
pub unsafe extern "C" fn linux_free_irq(irq: c_uint, name: *const c_char) -> c_int {
    if name.is_null() {
        return -14;
    }

    let dev_name = match unsafe { core::ffi::CStr::from_ptr(name) }.to_str() {
        Ok(s) => s,
        Err(_) => return -22,
    };


    match free_irq(irq as usize, dev_name) {
        Ok(()) => 0,
        Err(e) => -(e.as_i32()),
    }
}

