//! System call handler for `sys_reboot` (SYS_REBOOT = 169).
//!
//! Provides Linux-compatible `reboot(2)` handling including poweroff,
//! restart, halt, and CAD (Ctrl-Alt-Del) keystroke behavior configuration.

use crate::arch::syscall::SyscallFrame;
use crate::syscalls::{SyscallError, SyscallResult};
use core::sync::atomic::{AtomicBool, Ordering};

// ── Linux Reboot Magic Numbers ──────────────────────────────────────────────

/// First magic constant required by Linux `reboot(2)`.
pub const LINUX_REBOOT_MAGIC1: u32 = 0xfee1dead;

/// Valid second magic constants accepted by Linux `reboot(2)`.
pub const LINUX_REBOOT_MAGIC2: u32 = 672274793; // 0x28121969
pub const LINUX_REBOOT_MAGIC2A: u32 = 85072278; // 0x05121996
pub const LINUX_REBOOT_MAGIC2B: u32 = 369367492; // 0x16041998
pub const LINUX_REBOOT_MAGIC2C: u32 = 537993216; // 0x20112000

// ── Linux Reboot Commands ───────────────────────────────────────────────────

/// Restart system immediately.
pub const LINUX_REBOOT_CMD_RESTART: u32 = 0x01234567;

/// Halt system (stop CPU without cutting main power).
pub const LINUX_REBOOT_CMD_HALT: u32 = 0xcdef0123;

/// Enable Ctrl-Alt-Del keystroke to trigger reboot directly.
pub const LINUX_REBOOT_CMD_CAD_ON: u32 = 0x89abcdef;

/// Disable Ctrl-Alt-Del keystroke (sends SIGINT to init instead).
pub const LINUX_REBOOT_CMD_CAD_OFF: u32 = 0x00000000;

/// Cut main power and power down system.
pub const LINUX_REBOOT_CMD_POWER_OFF: u32 = 0x4321fedc;

/// Restart system with an optional reboot command string.
pub const LINUX_REBOOT_CMD_RESTART2: u32 = 0xa1b2c3d4;

/// Suspend system using software suspend (ACPI S4 / hibernation).
pub const LINUX_REBOOT_CMD_SW_SUSPEND: u32 = 0xd000fce2;

/// Restart system into a new kernel via kexec.
pub const LINUX_REBOOT_CMD_KEXEC: u32 = 0x45584543;

/// Global flag tracking whether CAD (Ctrl-Alt-Del) direct reboot is active.
static CAD_ENABLED: AtomicBool = AtomicBool::new(false);

/// Check whether Ctrl-Alt-Del should trigger direct reboot or signal init.
pub fn is_cad_enabled() -> bool {
    CAD_ENABLED.load(Ordering::Relaxed)
}

/// `sys_reboot` (SYS_REBOOT = 169)
///
/// Linux signature:
/// `int reboot(int magic, int magic2, int cmd, void *arg);`
pub fn sys_reboot(frame: &mut SyscallFrame) -> SyscallResult {
    let magic1 = frame.arg1() as u32;
    let magic2 = frame.arg2() as u32;
    let cmd = frame.arg3() as u32;

    // 1. Permission Check: Must be root / superuser (euid == 0).
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let is_privileged = {
        let proc = proc_arc.lock();
        proc.creds.is_privileged()
    };

    if !is_privileged {
        log::warn!("sys_reboot rejected: caller does not have superuser privileges");
        return Err(SyscallError::EPERM);
    }

    // 2. Magic Number Validation
    if magic1 != LINUX_REBOOT_MAGIC1 {
        log::warn!("sys_reboot rejected: invalid magic1 {:#x}", magic1);
        return Err(SyscallError::EINVAL);
    }

    let valid_magic2 = magic2 == LINUX_REBOOT_MAGIC2
        || magic2 == LINUX_REBOOT_MAGIC2A
        || magic2 == LINUX_REBOOT_MAGIC2B
        || magic2 == LINUX_REBOOT_MAGIC2C;

    if !valid_magic2 {
        log::warn!("sys_reboot rejected: invalid magic2 {:#x}", magic2);
        return Err(SyscallError::EINVAL);
    }

    // 3. Command Dispatch
    match cmd {
        LINUX_REBOOT_CMD_POWER_OFF => {
            log::info!("sys_reboot: LINUX_REBOOT_CMD_POWER_OFF received");
            crate::arch::poweroff()
        }
        LINUX_REBOOT_CMD_RESTART | LINUX_REBOOT_CMD_RESTART2 => {
            log::info!("sys_reboot: restart command {:#x} received", cmd);
            crate::arch::reboot()
        }
        LINUX_REBOOT_CMD_HALT => {
            log::info!("sys_reboot: LINUX_REBOOT_CMD_HALT received");
            crate::arch::power::idle()
        }
        LINUX_REBOOT_CMD_CAD_ON => {
            log::info!("sys_reboot: CAD enabled");
            CAD_ENABLED.store(true, Ordering::Relaxed);
            Ok(0)
        }
        LINUX_REBOOT_CMD_CAD_OFF => {
            log::info!("sys_reboot: CAD disabled");
            CAD_ENABLED.store(false, Ordering::Relaxed);
            Ok(0)
        }
        LINUX_REBOOT_CMD_SW_SUSPEND | LINUX_REBOOT_CMD_KEXEC => {
            log::warn!("sys_reboot: unsupported power state/command {:#x}", cmd);
            Err(SyscallError::ENOSYS)
        }
        unknown_cmd => {
            log::warn!("sys_reboot: unrecognized command {:#x}", unknown_cmd);
            Err(SyscallError::EINVAL)
        }
    }
}
