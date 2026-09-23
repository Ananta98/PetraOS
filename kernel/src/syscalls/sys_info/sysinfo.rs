//! `sys_sysinfo` system call handler.
//!
//! Handles:
//! - `sys_sysinfo` (SYS_SYSINFO = 99)

use crate::arch::syscall::syscall::SyscallFrame;
use crate::syscalls::{SyscallError, SyscallResult, UserPtr};

/// Linux x86_64 ABI compatible `sysinfo` structure layout (112 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct LinuxSysinfo {
    pub uptime: i64,
    pub loads: [u64; 3],
    pub totalram: u64,
    pub freeram: u64,
    pub sharedram: u64,
    pub bufferram: u64,
    pub totalswap: u64,
    pub freeswap: u64,
    pub procs: u16,
    pub pad: [u8; 6],
    pub totalhigh: u64,
    pub freehigh: u64,
    pub mem_unit: u32,
    pub _pad2: [u8; 4],
}

/// `sys_sysinfo` (SYS_SYSINFO = 99)
/// Return system information (uptime, RAM stats, process counts).
pub fn sys_sysinfo(frame: &mut SyscallFrame) -> SyscallResult {
    let ptr = UserPtr::<LinuxSysinfo>::from_u64(frame.arg1());
    if ptr.is_null() {
        return Err(SyscallError::EFAULT);
    }

    let uptime = (crate::clock::elapsed_ns() / crate::clock::NSEC_PER_SEC) as i64;
    let totalram = (crate::mm::alloc::PMM.total_pages() as u64) * crate::mm::alloc::PAGE_SIZE;
    let freeram = (crate::mm::alloc::PMM.free_pages_count() as u64) * crate::mm::alloc::PAGE_SIZE;
    let procs = crate::proc::all_processes().len() as u16;

    let info = LinuxSysinfo {
        uptime,
        loads: [0, 0, 0],
        totalram,
        freeram,
        sharedram: 0,
        bufferram: 0,
        totalswap: 0,
        freeswap: 0,
        procs,
        pad: [0; 6],
        totalhigh: 0,
        freehigh: 0,
        mem_unit: 1,
        _pad2: [0; 4],
    };

    ptr.write(info).ok_or(SyscallError::EFAULT)?;
    Ok(0)
}
