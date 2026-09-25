//! `sys_sigaltstack` system call handler.
//!
//! Handles:
//! - `sys_sigaltstack` (SYS_SIGALTSTACK = 131)

use crate::syscalls::{wrap_syscall, SyscallError, SyscallResult, UserPtr};

pub const SS_ONSTACK: i32 = 1;
pub const SS_DISABLE: i32 = 2;
pub const MINSIGSTKSZ: usize = 2048;

/// Linux x86_64 ABI compatible `stack_t` structure layout (24 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct StackT {
    pub ss_sp: u64,
    pub ss_flags: i32,
    pub _pad: i32,
    pub ss_size: usize,
}

/// `sys_sigaltstack` (SYS_SIGALTSTACK = 131)
/// Set and/or get signal alternate stack context.
#[wrap_syscall]
pub fn sys_sigaltstack(ss_ptr: UserPtr<StackT>, oss_ptr: UserPtr<StackT>) -> SyscallResult {
    // 1. If oss is requested, return current altstack state (default: disabled)
    if !oss_ptr.is_null() {
        let old = StackT {
            ss_sp: 0,
            ss_flags: SS_DISABLE,
            _pad: 0,
            ss_size: 0,
        };
        oss_ptr.write(old).ok_or(SyscallError::EFAULT)?;
    }

    // 2. If new ss is provided, validate parameters
    if !ss_ptr.is_null() {
        let ss = ss_ptr.read().ok_or(SyscallError::EFAULT)?;
        if (ss.ss_flags & !SS_DISABLE) != 0 {
            return Err(SyscallError::EINVAL);
        }
        if (ss.ss_flags & SS_DISABLE) == 0 && ss.ss_size < MINSIGSTKSZ {
            return Err(SyscallError::ENOMEM);
        }
    }

    Ok(0)
}
