//! sys_arch_prctl system call handler.

use super::*;
use crate::arch::syscall::SyscallFrame;
use crate::syscalls::{SyscallError, SyscallResult, UserPtr};


/// System call handler for `sys_arch_prctl(int code, unsigned long addr)`.
///
/// Configures architecture-specific thread context (e.g. FS/GS base for TLS/TCB).
pub fn sys_arch_prctl(frame: &mut SyscallFrame) -> SyscallResult {
    let code = frame.arg1();
    let addr = frame.arg2();

    match code {
        ARCH_SET_FS => {
            log::trace!("sys_arch_prctl: ARCH_SET_FS to {:#x}", addr);
            // SAFETY: Set IA32_FS_BASE for the current CPU/thread context.
            crate::arch::cpu::msr::write_fs_base(addr);
            if let Some(thread) = crate::proc::current_thread() {
                thread.lock().context.fs_base = addr;
            }
            Ok(0)
        }
        ARCH_GET_FS => {
            log::trace!("sys_arch_prctl: ARCH_GET_FS to {:#x}", addr);
            let ptr = UserPtr::<u64>::from_u64(addr);
            let fs_base = crate::proc::current_thread()
                .map(|t| t.lock().context.fs_base)
                .unwrap_or_else(crate::arch::cpu::msr::read_fs_base);
            ptr.write(fs_base).ok_or(SyscallError::EFAULT)?;
            Ok(0)
        }
        ARCH_SET_GS => {
            log::trace!("sys_arch_prctl: ARCH_SET_GS to {:#x}", addr);
            // SAFETY: Set IA32_GS_BASE for the current CPU/thread context.
            crate::arch::cpu::msr::write_gs_base(addr);
            if let Some(thread) = crate::proc::current_thread() {
                thread.lock().context.gs_base = addr;
            }
            Ok(0)
        }
        ARCH_GET_GS => {
            log::trace!("sys_arch_prctl: ARCH_GET_GS to {:#x}", addr);
            let ptr = UserPtr::<u64>::from_u64(addr);
            let gs_base = crate::arch::cpu::msr::read_gs_base();
            ptr.write(gs_base).ok_or(SyscallError::EFAULT)?;
            Ok(0)
        }
        _ => {
            log::warn!("sys_arch_prctl: invalid code {:#x}", code);
            Err(SyscallError::EINVAL)
        }
    }
}
