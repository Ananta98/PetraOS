use crate::syscall::{SyscallResult, to_continue_i32};
use crate::vm::vma::VmaManager;
use ostd::Error;
use ostd::arch::cpu::context::UserContext;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Utsname {
    pub sysname: [u8; 65],
    pub nodename: [u8; 65],
    pub release: [u8; 65],
    pub version: [u8; 65],
    pub machine: [u8; 65],
    pub domainname: [u8; 65],
}

impl Default for Utsname {
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

fn set_str(buf: &mut [u8; 65], s: &str) {
    let bytes = s.as_bytes();
    let len = bytes.len().min(64);
    buf[..len].copy_from_slice(&bytes[..len]);
    buf[len] = 0;
}

#[repr(C)]
#[derive(Default, Copy, Clone)]
pub struct Sysinfo {
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
    pub totalhigh: u64,
    pub freehigh: u64,
    pub mem_unit: u32,
    pub _f: [u8; 0],
}

#[repr(C)]
#[derive(Default, Copy, Clone)]
pub struct Rlimit64 {
    pub rlim_cur: u64,
    pub rlim_max: u64,
}

/// `uname()` — SYS_uname = 63
pub fn syscall_uname(
    arg0: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    if arg0 == 0 {
        return to_continue_i32(Err(Error::InvalidArgs));
    }
    let mut uts = Utsname::default();
    set_str(&mut uts.sysname, "Linux");
    set_str(&mut uts.nodename, "petra");
    set_str(&mut uts.release, "6.8.0-petra");
    set_str(&mut uts.version, "#1 SMP SafeRust PetraOS");
    set_str(&mut uts.machine, "x86_64");
    set_str(&mut uts.domainname, "(none)");

    let mut buf = [0u8; 390];
    buf[0..65].copy_from_slice(&uts.sysname);
    buf[65..130].copy_from_slice(&uts.nodename);
    buf[130..195].copy_from_slice(&uts.release);
    buf[195..260].copy_from_slice(&uts.version);
    buf[260..325].copy_from_slice(&uts.machine);
    buf[325..390].copy_from_slice(&uts.domainname);

    to_continue_i32(vm.copy_to_user(arg0, &buf).map(|_| 0))
}

/// `sysinfo()` — SYS_sysinfo = 99
pub fn syscall_sysinfo(
    arg0: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    if arg0 == 0 {
        return to_continue_i32(Err(Error::InvalidArgs));
    }
    let info = Sysinfo {
        uptime: 3600,
        loads: [100, 100, 100],
        totalram: 8 * 1024 * 1024 * 1024,
        freeram: 6 * 1024 * 1024 * 1024,
        sharedram: 128 * 1024 * 1024,
        bufferram: 256 * 1024 * 1024,
        totalswap: 2 * 1024 * 1024 * 1024,
        freeswap: 2 * 1024 * 1024 * 1024,
        procs: 16,
        pad: 0,
        totalhigh: 0,
        freehigh: 0,
        mem_unit: 1,
        _f: [],
    };

    let mut buf = [0u8; 112];
    buf[0..8].copy_from_slice(&info.uptime.to_ne_bytes());
    buf[8..16].copy_from_slice(&info.loads[0].to_ne_bytes());
    buf[16..24].copy_from_slice(&info.loads[1].to_ne_bytes());
    buf[24..32].copy_from_slice(&info.loads[2].to_ne_bytes());
    buf[32..40].copy_from_slice(&info.totalram.to_ne_bytes());
    buf[40..48].copy_from_slice(&info.freeram.to_ne_bytes());
    buf[48..56].copy_from_slice(&info.sharedram.to_ne_bytes());
    buf[56..64].copy_from_slice(&info.bufferram.to_ne_bytes());
    buf[64..72].copy_from_slice(&info.totalswap.to_ne_bytes());
    buf[72..80].copy_from_slice(&info.freeswap.to_ne_bytes());
    buf[80..82].copy_from_slice(&info.procs.to_ne_bytes());
    buf[82..84].copy_from_slice(&info.pad.to_ne_bytes());
    buf[84..92].copy_from_slice(&info.totalhigh.to_ne_bytes());
    buf[92..100].copy_from_slice(&info.freehigh.to_ne_bytes());
    buf[100..104].copy_from_slice(&info.mem_unit.to_ne_bytes());

    to_continue_i32(vm.copy_to_user(arg0, &buf).map(|_| 0))
}

/// `getrlimit()` — SYS_getrlimit = 97
pub fn syscall_getrlimit(
    _resource: usize,
    rlim_ptr: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    if rlim_ptr == 0 {
        return to_continue_i32(Err(Error::InvalidArgs));
    }
    let rlim = Rlimit64 {
        rlim_cur: 1024,
        rlim_max: 4096,
    };

    let mut buf = [0u8; 16];
    buf[0..8].copy_from_slice(&rlim.rlim_cur.to_ne_bytes());
    buf[8..16].copy_from_slice(&rlim.rlim_max.to_ne_bytes());

    to_continue_i32(vm.copy_to_user(rlim_ptr, &buf).map(|_| 0))
}

/// `prlimit64()` — SYS_prlimit64 = 302
pub fn syscall_prlimit64(
    _pid: usize,
    resource: usize,
    _new_rlim: usize,
    old_rlim: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    ctx: &mut UserContext,
) -> SyscallResult {
    if old_rlim != 0 {
        syscall_getrlimit(resource, old_rlim, 0, 0, 0, 0, vm, ctx)
    } else {
        to_continue_i32(Ok(0))
    }
}
