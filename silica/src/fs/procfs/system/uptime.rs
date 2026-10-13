// SPDX-License-Identifier: GPL-2.0

//! `/proc/uptime` generator.

use alloc::{format, vec::Vec};

pub fn generate_uptime() -> Vec<u8> {
    let uptime = crate::sched::now() / crate::sched::TIMER_HZ;
    format!("{}.00 {}.00\n", uptime, uptime).into_bytes()
}
