// SPDX-License-Identifier: GPL-2.0

//! `/proc/cmdline` generator.

use alloc::vec::Vec;

pub fn generate_cmdline() -> Vec<u8> {
    b"console=tty0 root=/dev/ram0 init=/bin/sh\n".to_vec()
}
