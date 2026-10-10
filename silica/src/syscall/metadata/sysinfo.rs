// SPDX-License-Identifier: GPL-2.0

//! `sysinfo(2)` system call (Syscall #99 on x86_64).

use ostd::arch::cpu::context::UserContext;

use crate::{
    api::errno::{Errno, Result},
    proc::thread::Thread,
    utils::userspace::user_ptr::UserPtr,
};

/// Linux x86_64 compatible `struct sysinfo` (112 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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
        let mut buf = [0u8; 112];
        buf[0..8].copy_from_slice(&self.uptime.to_ne_bytes());
        buf[8..16].copy_from_slice(&self.loads[0].to_ne_bytes());
        buf[16..24].copy_from_slice(&self.loads[1].to_ne_bytes());
        buf[24..32].copy_from_slice(&self.loads[2].to_ne_bytes());
        buf[32..40].copy_from_slice(&self.totalram.to_ne_bytes());
        buf[40..48].copy_from_slice(&self.freeram.to_ne_bytes());
        buf[48..56].copy_from_slice(&self.sharedram.to_ne_bytes());
        buf[56..64].copy_from_slice(&self.bufferram.to_ne_bytes());
        buf[64..72].copy_from_slice(&self.totalswap.to_ne_bytes());
        buf[72..80].copy_from_slice(&self.freeswap.to_ne_bytes());
        buf[80..82].copy_from_slice(&self.procs.to_ne_bytes());
        buf[82..84].copy_from_slice(&self.pad.to_ne_bytes());
        buf[84..88].copy_from_slice(&self._pad2.to_ne_bytes());
        buf[88..96].copy_from_slice(&self.totalhigh.to_ne_bytes());
        buf[96..104].copy_from_slice(&self.freehigh.to_ne_bytes());
        buf[104..108].copy_from_slice(&self.mem_unit.to_ne_bytes());
        buf[108..112].copy_from_slice(&self._f);
        buf
    }
}

/// Retrieves overall system statistics.
pub fn sys_sysinfo(info_ptr: usize, _ctx: &mut UserContext) -> Result<usize> {
    if info_ptr == 0 {
        return Err(Errno::EFAULT);
    }

    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();

    let uptime = (crate::sched::now() / crate::sched::TIMER_HZ) as i64;
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
    for p in crate::proc::tree::allproc_iter() {
        mapped_ram = mapped_ram.saturating_add(p.vmspace().total_mapped_size() as u64);
    }

    let free_ram = total_ram
        .saturating_sub(mapped_ram)
        .saturating_sub(32 * 1024 * 1024)
        .max(total_ram / 4);

    let procs = crate::proc::tree::allproc_count() as u16;

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

    UserPtr::new(info_ptr).write_bytes(&vmar, &info.to_bytes())?;

    Ok(0)
}
