//! `sys_arch_prctl` system call handler for x86_64 architecture-specific thread context.
//!
//! Configures architecture-specific thread state (FS/GS base registers for TLS/TCB).

use crate::arch::syscall::SyscallFrame;
use crate::syscalls::{SyscallError, SyscallResult, UserPtr, USER_SPACE_MAX_ADDR};

pub const ARCH_SET_GS: u64 = 0x1001;
pub const ARCH_SET_FS: u64 = 0x1002;
pub const ARCH_GET_FS: u64 = 0x1003;
pub const ARCH_GET_GS: u64 = 0x1004;

/// System call handler for `sys_arch_prctl(int code, unsigned long addr)`.
///
/// Configures architecture-specific thread context (e.g. FS/GS base for TLS/TCB).
///
/// # Subcommands
/// - `ARCH_SET_FS`: Sets 64-bit FS base address.
/// - `ARCH_GET_FS`: Reads 64-bit FS base address and stores into user memory at `addr`.
/// - `ARCH_SET_GS`: Sets 64-bit GS base address (stored in `IA32_KERNEL_GS_BASE` during kernel execution).
/// - `ARCH_GET_GS`: Reads 64-bit GS base address and stores into user memory at `addr`.
pub fn sys_arch_prctl(frame: &mut SyscallFrame) -> SyscallResult {
    let code = frame.arg1();
    let addr = frame.arg2();

    match code {
        ARCH_SET_FS => {
            log::trace!("sys_arch_prctl: ARCH_SET_FS to {:#x}", addr);
            if addr > USER_SPACE_MAX_ADDR {
                return Err(SyscallError::EPERM);
            }
            // SAFETY: addr is validated to reside strictly within canonical user-space memory bounds.
            crate::arch::cpu::msr::write_fs_base(addr);
            if let Some(thread) = crate::proc::current_thread() {
                thread.lock().context.fs_base = addr;
            }
            Ok(0)
        }
        ARCH_GET_FS => {
            log::trace!("sys_arch_prctl: ARCH_GET_FS to {:#x}", addr);
            if addr % (core::mem::align_of::<u64>() as u64) != 0 {
                return Err(SyscallError::EFAULT);
            }
            let ptr = UserPtr::<u64>::from_u64(addr);
            let fs_base = crate::proc::current_thread()
                .map(|t| t.lock().context.fs_base)
                .unwrap_or_else(crate::arch::cpu::msr::read_fs_base);
            ptr.write(fs_base).ok_or(SyscallError::EFAULT)?;
            Ok(0)
        }
        ARCH_SET_GS => {
            log::trace!("sys_arch_prctl: ARCH_SET_GS to {:#x}", addr);
            if addr > USER_SPACE_MAX_ADDR {
                return Err(SyscallError::EPERM);
            }
            // SAFETY: In kernel mode (after swapgs), user GS base resides in IA32_KERNEL_GS_BASE.
            // Writing here ensures swapgs will restore it to IA32_GS_BASE on return to user space.
            crate::arch::cpu::msr::write_kernel_gs_base(addr);
            if let Some(thread) = crate::proc::current_thread() {
                thread.lock().context.gs_base = addr;
            }
            Ok(0)
        }
        ARCH_GET_GS => {
            log::trace!("sys_arch_prctl: ARCH_GET_GS to {:#x}", addr);
            if addr % (core::mem::align_of::<u64>() as u64) != 0 {
                return Err(SyscallError::EFAULT);
            }
            let ptr = UserPtr::<u64>::from_u64(addr);
            let gs_base = crate::proc::current_thread()
                .map(|t| t.lock().context.gs_base)
                .unwrap_or_else(crate::arch::cpu::msr::read_kernel_gs_base);
            ptr.write(gs_base).ok_or(SyscallError::EFAULT)?;
            Ok(0)
        }
        _ => {
            log::warn!("sys_arch_prctl: invalid code {:#x}", code);
            Err(SyscallError::EINVAL)
        }
    }
}

