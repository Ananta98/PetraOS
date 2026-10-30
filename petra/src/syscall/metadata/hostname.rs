// SPDX-License-Identifier: GPL-2.0

//! System calls `sethostname(2)` and `setdomainname(2)` (Syscalls #170 and #171 on x86_64).

use alloc::string::String;
use ostd::arch::cpu::context::UserContext;

use crate::{
    fs::procfs::sys::{set_domainname, set_hostname},
    proc::thread::Thread,
    uapi::errno::{Errno, Result},
    utils::userspace::UserPtr,
};

const MAX_HOST_NAME_LEN: usize = 64;

/// System call `sethostname(const char *name, size_t len)` (Syscall #170).
pub fn sys_sethostname(name_ptr: usize, len: usize, _ctx: &mut UserContext) -> Result<usize> {
    if len > MAX_HOST_NAME_LEN {
        return Err(Errno::EINVAL);
    }
    if name_ptr == 0 {
        return Err(Errno::EFAULT);
    }
    let proc = Thread::proc_or_esrch()?;
    if !proc.cred().is_superuser() {
        return Err(Errno::EPERM);
    }
    let mut buf = alloc::vec![0u8; len];
    let vmar = proc.vmspace();
    UserPtr::new(name_ptr).read_bytes(&vmar, &mut buf)?;

    let name = core::str::from_utf8(&buf)
        .map_err(|_| Errno::EINVAL)?
        .trim();
    set_hostname(String::from(name));
    Ok(0)
}

/// System call `setdomainname(const char *name, size_t len)` (Syscall #171).
pub fn sys_setdomainname(name_ptr: usize, len: usize, _ctx: &mut UserContext) -> Result<usize> {
    if len > MAX_HOST_NAME_LEN {
        return Err(Errno::EINVAL);
    }
    if name_ptr == 0 {
        return Err(Errno::EFAULT);
    }
    let proc = Thread::proc_or_esrch()?;
    if !proc.cred().is_superuser() {
        return Err(Errno::EPERM);
    }
    let mut buf = alloc::vec![0u8; len];
    let vmar = proc.vmspace();
    UserPtr::new(name_ptr).read_bytes(&vmar, &mut buf)?;

    let name = core::str::from_utf8(&buf)
        .map_err(|_| Errno::EINVAL)?
        .trim();
    set_domainname(String::from(name));
    Ok(0)
}
