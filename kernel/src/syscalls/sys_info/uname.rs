//! sys_uname system call handler.

use super::*;
use crate::syscalls::{wrap_syscall, SyscallError, SyscallResult, UserPtr};

/// `sys_uname` (SYS_UNAME = 63)
/// Get name and information about current kernel.
#[wrap_syscall]
pub fn sys_uname(buf: UserPtr<UtsName>) -> SyscallResult {
    let mut uts = UtsName::default();
    set_bytes(&mut uts.sysname, b"PetraOS");
    set_bytes(&mut uts.nodename, b"petra");
    set_bytes(&mut uts.release, b"2026");
    set_bytes(&mut uts.version, b"0.1.0");
    set_bytes(&mut uts.machine, b"x86_64");
    set_bytes(&mut uts.domainname, b"localdomain");

    buf.write(uts).ok_or(SyscallError::EFAULT)?;

    Ok(0)
}
