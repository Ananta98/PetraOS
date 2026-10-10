// SPDX-License-Identifier: GPL-2.0

//! Platform Reboot ABI definitions.
//!
//! Defines Linux magic numbers and command codes passed to `reboot(2)`.

/// Primary magic number required by Linux `reboot(2)` (`0xfee1dead`).
pub const LINUX_REBOOT_MAGIC1: i32 = -18751827;

/// Secondary magic number 1 (0x28121969).
pub const LINUX_REBOOT_MAGIC2: i32 = 672274793;

/// Secondary magic number 2A (0x05121996).
pub const LINUX_REBOOT_MAGIC2A: i32 = 85072278;

/// Secondary magic number 2B (0x16041998).
pub const LINUX_REBOOT_MAGIC2B: i32 = 369367448;

/// Secondary magic number 2C (0x20112000).
pub const LINUX_REBOOT_MAGIC2C: i32 = 537993216;

/// Restart system.
pub const LINUX_REBOOT_CMD_RESTART: u32 = 0x01234567;

/// Halt system.
pub const LINUX_REBOOT_CMD_HALT: u32 = 0xcdef0123;

/// Enable reboot on Ctrl-Alt-Del key sequence.
pub const LINUX_REBOOT_CMD_CAD_ON: u32 = 0x89abcdef;

/// Disable reboot on Ctrl-Alt-Del key sequence.
pub const LINUX_REBOOT_CMD_CAD_OFF: u32 = 0x00000000;

/// Power off system.
pub const LINUX_REBOOT_CMD_POWER_OFF: u32 = 0x4321fedc;

/// Restart system with a command string.
pub const LINUX_REBOOT_CMD_RESTART2: u32 = 0xa1b2c3d4;

/// Suspend system using software suspend (swsusp).
pub const LINUX_REBOOT_CMD_SW_SUSPEND: u32 = 0xd000fce2;

/// Restart system into a new kernel via kexec.
pub const LINUX_REBOOT_CMD_KEXEC: u32 = 0x45584543;

/// Validates whether the given `magic1` and `magic2` pair matches Linux requirements.
pub const fn is_valid_reboot_magic(magic1: i32, magic2: i32) -> bool {
    if magic1 != LINUX_REBOOT_MAGIC1 {
        return false;
    }
    matches!(
        magic2,
        LINUX_REBOOT_MAGIC2 | LINUX_REBOOT_MAGIC2A | LINUX_REBOOT_MAGIC2B | LINUX_REBOOT_MAGIC2C
    )
}
