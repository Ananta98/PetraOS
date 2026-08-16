//! System call handler for `arch_prctl` (x86_64 architecture-specific control).

use crate::arch::syscall::SyscallFrame;
use crate::syscalls::{SyscallError, SyscallResult, is_user_ptr_valid};

pub const ARCH_SET_GS: u64 = 0x1001;
pub const ARCH_SET_FS: u64 = 0x1002;
pub const ARCH_GET_FS: u64 = 0x1003;
pub const ARCH_GET_GS: u64 = 0x1004;

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
            if !is_user_ptr_valid(addr, 8) {
                return Err(SyscallError::EFAULT);
            }
            let fs_base = crate::proc::current_thread()
                .map(|t| t.lock().context.fs_base)
                .unwrap_or_else(crate::arch::cpu::msr::read_fs_base);
            // SAFETY: Validated user memory pointer.
            unsafe {
                core::ptr::write_unaligned(addr as *mut u64, fs_base);
            }
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
            if !is_user_ptr_valid(addr, 8) {
                return Err(SyscallError::EFAULT);
            }
            let gs_base = crate::arch::cpu::msr::read_gs_base();
            // SAFETY: Validated user memory pointer.
            unsafe {
                core::ptr::write_unaligned(addr as *mut u64, gs_base);
            }
            Ok(0)
        }
        _ => {
            log::warn!("sys_arch_prctl: invalid code {:#x}", code);
            Err(SyscallError::EINVAL)
        }
    }
}
