// SPDX-License-Identifier: GPL-2.0

//! `uname(2)` system call (Syscall #63 on x86_64).

use ostd::arch::cpu::context::UserContext;

use crate::{
    api::errno::Result,
    fs::procfs::sys::get_hostname,
    proc::thread::Thread,
    utils::userspace::user_ptr::UserPtr,
};

pub const UTS_LEN: usize = 65;

/// Linux x86_64 compatible `struct utsname` (390 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct UtsName {
    pub sysname: [u8; UTS_LEN],
    pub nodename: [u8; UTS_LEN],
    pub release: [u8; UTS_LEN],
    pub version: [u8; UTS_LEN],
    pub machine: [u8; UTS_LEN],
    pub domainname: [u8; UTS_LEN],
}

impl Default for UtsName {
    fn default() -> Self {
        Self::new()
    }
}

impl UtsName {
    pub fn new() -> Self {
        let mut uts = Self {
            sysname: [0u8; UTS_LEN],
            nodename: [0u8; UTS_LEN],
            release: [0u8; UTS_LEN],
            version: [0u8; UTS_LEN],
            machine: [0u8; UTS_LEN],
            domainname: [0u8; UTS_LEN],
        };

        copy_cstr(&mut uts.sysname, b"PetraOS");
        let nodename = get_hostname();
        copy_cstr(&mut uts.nodename, nodename.as_bytes());
        copy_cstr(&mut uts.release, b"6.6.0-petraos");
        copy_cstr(&mut uts.version, b"#1 SMP PetraOS 0.1.0");
        copy_cstr(&mut uts.machine, b"x86_64");
        copy_cstr(&mut uts.domainname, b"(none)");

        uts
    }

    pub fn to_bytes(&self) -> [u8; UTS_LEN * 6] {
        let mut buf = [0u8; UTS_LEN * 6];
        buf[0..65].copy_from_slice(&self.sysname);
        buf[65..130].copy_from_slice(&self.nodename);
        buf[130..195].copy_from_slice(&self.release);
        buf[195..260].copy_from_slice(&self.version);
        buf[260..325].copy_from_slice(&self.machine);
        buf[325..390].copy_from_slice(&self.domainname);
        buf
    }
}

fn copy_cstr(dst: &mut [u8; UTS_LEN], src: &[u8]) {
    let len = src.len().min(UTS_LEN - 1);
    dst[..len].copy_from_slice(&src[..len]);
    dst[len] = 0;
}

/// Retrieves system identification information.
pub fn sys_uname(buf_ptr: usize, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();

    let uts = UtsName::new();
    UserPtr::new(buf_ptr).write_bytes(&vmar, &uts.to_bytes())?;

    Ok(0)
}
