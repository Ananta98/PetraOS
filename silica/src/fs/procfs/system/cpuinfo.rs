// SPDX-License-Identifier: GPL-2.0

//! `/proc/cpuinfo` generator.

use alloc::vec::Vec;

pub fn generate_cpuinfo() -> Vec<u8> {
    crate::arch::cpuinfo::generate_cpuinfo()
}
