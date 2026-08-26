//! sys_uname system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult, UserPtr};
use crate::arch::syscall::syscall::SyscallFrame;


/// `sys_uname` (SYS_UNAME = 63)
/// Get name and information about current kernel.
pub fn sys_uname(frame: &mut SyscallFrame) -> SyscallResult {
    let buf = UserPtr::<UtsName>::from_u64(frame.arg1());

    let mut uts = UtsName::default();
    set_bytes(&mut uts.sysname, b"PetraOS");
    set_bytes(&mut uts.nodename, b"petra");
    set_bytes(&mut uts.release, b"0.1.0");
    set_bytes(&mut uts.version, b"PetraOS Kernel v0.1.0 no_std");
    set_bytes(&mut uts.machine, b"x86_64");
    set_bytes(&mut uts.domainname, b"localdomain");

    buf.write(uts).ok_or(SyscallError::EFAULT)?;

    Ok(0)
}
