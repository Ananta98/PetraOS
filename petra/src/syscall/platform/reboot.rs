// SPDX-License-Identifier: GPL-2.0

//! Reboot and power management system calls.

use crate::uapi::errno::{Errno, Result};
use crate::uapi::reboot::*;
use ostd::arch::cpu::context::UserContext;

use crate::proc::thread::Thread;

/// Handles `reboot(2)` system call (`SYS_REBOOT = 169`).
///
/// Restarts, powers off, or manages CAD behavior depending on `cmd`.
/// Requires superuser privileges and valid Linux reboot magic numbers.
pub fn sys_reboot(
    magic1: i32,
    magic2: i32,
    cmd: u32,
    _arg: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let caller_proc = Thread::proc_or_esrch()?;
    if !caller_proc.cred().is_superuser() {
        return Err(Errno::EPERM);
    }

    if !is_valid_reboot_magic(magic1, magic2) {
        return Err(Errno::EINVAL);
    }

    match cmd {
        LINUX_REBOOT_CMD_RESTART | LINUX_REBOOT_CMD_RESTART2 => {
            ostd::power::restart(ostd::power::ExitCode::Success);
        }
        LINUX_REBOOT_CMD_POWER_OFF | LINUX_REBOOT_CMD_HALT => {
            ostd::power::poweroff(ostd::power::ExitCode::Success);
        }
        LINUX_REBOOT_CMD_CAD_ON | LINUX_REBOOT_CMD_CAD_OFF => {
            // Acknowledge CAD configuration request successfully.
            Ok(0)
        }
        LINUX_REBOOT_CMD_SW_SUSPEND | LINUX_REBOOT_CMD_KEXEC => Err(Errno::ENOTSUP),
        _ => Err(Errno::EINVAL),
    }
}
