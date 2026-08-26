use super::{SyscallError, SyscallResult, UserPtr};
use crate::arch::syscall::syscall::SyscallFrame;

// ── Modular syscall submodules ──────────────────────────────────────────
pub mod uname;

pub use uname::sys_uname;


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

pub(crate) fn set_bytes(dst: &mut [u8; 65], src: &[u8]) {
    let len = core::cmp::min(src.len(), 64);
    dst[..len].copy_from_slice(&src[..len]);
    dst[len] = 0;
}
