// SPDX-License-Identifier: GPL-2.0

//! `/proc/version` generator.

use alloc::vec::Vec;

pub fn generate_version() -> Vec<u8> {
    b"Linux version 6.6.0-silica (petraos@build) (rustc 1.85.0) #1 PREEMPT Mon Oct 12 2026\n".to_vec()
}
