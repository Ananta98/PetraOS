// SPDX-License-Identifier: GPL-2.0

//! `sysinfo(2)` system call (Syscall #99 on x86_64).

use ostd::arch::cpu::context::UserContext;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use crate::{
    proc::Thread,
    uapi::errno::Result,
    utils::UserPtr,
};

/// Linux x86_64 compatible `struct sysinfo` (112 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct SysInfo {
    pub uptime: i64,
    pub loads: [u64; 3],
    pub totalram: u64,
    pub freeram: u64,
    pub sharedram: u64,
    pub bufferram: u64,
    pub totalswap: u64,
    pub freeswap: u64,
    pub procs: u16,
    pub pad: u16,
    pub _pad2: u32,
    pub totalhigh: u64,
    pub freehigh: u64,
    pub mem_unit: u32,
    pub _f: [u8; 4],
}

impl SysInfo {
    pub fn to_bytes(&self) -> [u8; 112] {
        zerocopy::transmute!(*self)
    }
}

/// Retrieves overall system statistics.
pub fn sys_sysinfo(info_ptr: usize, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();

    let uptime = crate::time::uptime_secs() as i64;
    let load = crate::sched::loadavg();
    // In Linux sysinfo, loads are scaled by 2^16 (SI_LOAD_SHIFT = 16)
    let loads = [load.one >> 8, load.five >> 8, load.fifteen >> 8];

    // Compute total RAM from memory regions
    let boot = ostd::boot::boot_info();
    let mut total_ram: u64 = 0;
    for region in &boot.memory_regions {
        if region.typ().is_physical() {
            total_ram = total_ram.saturating_add(region.len() as u64);
        }
    }
    if total_ram == 0 {
        total_ram = 512 * 1024 * 1024;
    }

    // Compute approximate mapped RAM from all processes
    let mut mapped_ram: u64 = 0;
    crate::proc::allproc_for_each(|p| {
        mapped_ram = mapped_ram.saturating_add(p.vmspace().total_mapped_size() as u64);
    });

    let free_ram = total_ram
        .saturating_sub(mapped_ram)
        .saturating_sub(32 * 1024 * 1024)
        .max(total_ram / 4);

    let procs = crate::proc::allproc_count() as u16;

    let info = SysInfo {
        uptime,
        loads,
        totalram: total_ram,
        freeram: free_ram,
        sharedram: 0,
        bufferram: 0,
        totalswap: 0,
        freeswap: 0,
        procs,
        pad: 0,
        _pad2: 0,
        totalhigh: 0,
        freehigh: 0,
        mem_unit: 1, // Unit size in bytes
        _f: [0u8; 4],
    };

    UserPtr::new(info_ptr).write_val(&vmar, info)?;

    Ok(0)
}
