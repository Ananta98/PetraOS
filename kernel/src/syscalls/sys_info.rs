use super::{is_user_ptr_valid, SyscallError, SyscallResult};
use crate::arch::syscall::syscall::SyscallFrame;

/// x86_64 Linux ABI compatible utsname structure layout.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct UtsName {
    pub sysname: [u8; 65],
    pub nodename: [u8; 65],
    pub release: [u8; 65],
    pub version: [u8; 65],
    pub machine: [u8; 65],
    pub domainname: [u8; 65],
}

impl Default for UtsName {
    fn default() -> Self {
        Self {
            sysname: [0; 65],
            nodename: [0; 65],
            release: [0; 65],
            version: [0; 65],
            machine: [0; 65],
            domainname: [0; 65],
        }
    }
}

fn set_bytes(dst: &mut [u8; 65], src: &[u8]) {
    let len = core::cmp::min(src.len(), 64);
    dst[..len].copy_from_slice(&src[..len]);
    dst[len] = 0;
}

/// `sys_uname` (SYS_UNAME = 63)
/// Get name and information about current kernel.
pub fn sys_uname(frame: &mut SyscallFrame) -> SyscallResult {
    let buf = frame.arg1() as *mut UtsName;
    if !is_user_ptr_valid(buf as u64, core::mem::size_of::<UtsName>()) {
        return Err(SyscallError::EFAULT);
    }

    let mut uts = UtsName::default();
    set_bytes(&mut uts.sysname, b"PetraOS");
    set_bytes(&mut uts.nodename, b"petra");
    set_bytes(&mut uts.release, b"0.1.0");
    set_bytes(&mut uts.version, b"PetraOS Kernel v0.1.0 no_std");
    set_bytes(&mut uts.machine, b"x86_64");
    set_bytes(&mut uts.domainname, b"localdomain");

    // SAFETY: Writing UtsName struct to user pointer after bounds validation.
    unsafe {
        core::ptr::write_unaligned(buf, uts);
    }

    Ok(0)
}
