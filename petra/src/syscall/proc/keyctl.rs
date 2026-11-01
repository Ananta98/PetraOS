// SPDX-License-Identifier: GPL-2.0

//! `keyctl(2)` system call (Syscall #250 on x86_64).
//!
//! Provides key management operations. Emulates basic session keyring
//! operations returning dummy keyring identifiers so that systemd and
//! session managers operate smoothly without fatal errors.

use ostd::arch::cpu::context::UserContext;

use crate::uapi::errno::{Errno, Result};

const KEYCTL_GET_KEYRING_ID: i32 = 0;
const KEYCTL_JOIN_SESSION_KEYRING: i32 = 1;
const KEYCTL_CLEAR: i32 = 7;
const KEYCTL_SESSION_TO_PARENT: i32 = 18;

/// System call `keyctl(int operation, ...)` (Syscall #250).
pub fn sys_keyctl(
    operation: i32,
    _arg2: usize,
    _arg3: usize,
    _arg4: usize,
    _arg5: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    match operation {
        KEYCTL_GET_KEYRING_ID => Ok(1),
        KEYCTL_JOIN_SESSION_KEYRING => Ok(1),
        KEYCTL_CLEAR | KEYCTL_SESSION_TO_PARENT => Ok(0),
        _ => Err(Errno::ENOSYS),
    }
}
