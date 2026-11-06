// SPDX-License-Identifier: GPL-2.0

//! Capability system calls (`capget` #125, `capset` #126).

use ostd::arch::cpu::context::UserContext;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use crate::{
    proc::thread::Thread,
    uapi::errno::{Errno, Result},
    utils::UserPtr,
};

/// Linux capability ABI versions.
pub const LINUX_CAPABILITY_VERSION_1: u32 = 0x1998_0330;
pub const LINUX_CAPABILITY_VERSION_2: u32 = 0x2007_1026;
pub const LINUX_CAPABILITY_VERSION_3: u32 = 0x2008_0522;

/// Linux `cap_user_header_t` structure.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct CapUserHeader {
    pub version: u32,
    pub pid: i32,
}

/// Linux `cap_user_data_t` structure.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct CapUserData {
    pub effective: u32,
    pub permitted: u32,
    pub inheritable: u32,
}

/// System call `capget(cap_user_header_t hdrp, cap_user_data_t datap)` (Syscall #125).
pub fn sys_capget(hdrp: usize, datap: usize, _ctx: &mut UserContext) -> Result<usize> {
    if hdrp == 0 {
        return Err(Errno::EFAULT);
    }

    let caller_td = Thread::current().ok_or(Errno::ESRCH)?;
    let caller_proc = caller_td.proc().ok_or(Errno::ESRCH)?;
    let vmar = caller_proc.vmspace();

    let hdr = UserPtr::new(hdrp).read_val::<CapUserHeader>(&vmar)?;
    let version = hdr.version;
    let pid = hdr.pid;

    // Standard libcap version probe: datap == NULL writes preferred version and returns 0.
    if datap == 0 {
        UserPtr::new(hdrp).write_val(&vmar, LINUX_CAPABILITY_VERSION_3)?;
        return Ok(0);
    }

    if version != LINUX_CAPABILITY_VERSION_1
        && version != LINUX_CAPABILITY_VERSION_2
        && version != LINUX_CAPABILITY_VERSION_3
    {
        UserPtr::new(hdrp).write_val(&vmar, LINUX_CAPABILITY_VERSION_3)?;
        return Err(Errno::EINVAL);
    }

    if pid < 0 {
        return Err(Errno::EINVAL);
    }

    let target_proc = if pid == 0 || pid == caller_proc.pid.as_u32() as i32 {
        caller_proc.clone()
    } else {
        let target_pid = crate::proc::Pid::from_u32(pid as u32).ok_or(Errno::EINVAL)?;
        crate::proc::allproc_find(target_pid).ok_or(Errno::ESRCH)?
    };

    let (eff, perm, inh) = {
        let inner = target_proc.inner.lock();
        (inner.cap_effective, inner.cap_permitted, inner.cap_inheritable)
    };

    if version == LINUX_CAPABILITY_VERSION_1 {
        let data = CapUserData {
            effective: eff as u32,
            permitted: perm as u32,
            inheritable: inh as u32,
        };
        UserPtr::new(datap).write_val(&vmar, data)?;
    } else {
        let data = [
            CapUserData {
                effective: (eff & 0xffff_ffff) as u32,
                permitted: (perm & 0xffff_ffff) as u32,
                inheritable: (inh & 0xffff_ffff) as u32,
            },
            CapUserData {
                effective: ((eff >> 32) & 0xffff_ffff) as u32,
                permitted: ((perm >> 32) & 0xffff_ffff) as u32,
                inheritable: ((inh >> 32) & 0xffff_ffff) as u32,
            },
        ];
        UserPtr::new(datap).write_val(&vmar, data)?;
    }

    Ok(0)
}

/// System call `capset(cap_user_header_t hdrp, const cap_user_data_t datap)` (Syscall #126).
pub fn sys_capset(hdrp: usize, datap: usize, _ctx: &mut UserContext) -> Result<usize> {
    if hdrp == 0 || datap == 0 {
        return Err(Errno::EFAULT);
    }

    let caller_td = Thread::current().ok_or(Errno::ESRCH)?;
    let caller_proc = caller_td.proc().ok_or(Errno::ESRCH)?;
    let vmar = caller_proc.vmspace();

    let hdr = UserPtr::new(hdrp).read_val::<CapUserHeader>(&vmar)?;
    let version = hdr.version;
    let pid = hdr.pid;

    if version != LINUX_CAPABILITY_VERSION_1
        && version != LINUX_CAPABILITY_VERSION_2
        && version != LINUX_CAPABILITY_VERSION_3
    {
        UserPtr::new(hdrp).write_val(&vmar, LINUX_CAPABILITY_VERSION_3)?;
        return Err(Errno::EINVAL);
    }

    // In Linux, capset can only affect calling thread/process
    if pid != 0 && pid != caller_proc.pid.as_u32() as i32 {
        return Err(Errno::EPERM);
    }

    let (new_eff, new_perm, new_inh) = if version == LINUX_CAPABILITY_VERSION_1 {
        let data = UserPtr::new(datap).read_val::<CapUserData>(&vmar)?;
        (data.effective as u64, data.permitted as u64, data.inheritable as u64)
    } else {
        let data = UserPtr::new(datap).read_val::<[CapUserData; 2]>(&vmar)?;
        (
            (data[0].effective as u64) | ((data[1].effective as u64) << 32),
            (data[0].permitted as u64) | ((data[1].permitted as u64) << 32),
            (data[0].inheritable as u64) | ((data[1].inheritable as u64) << 32),
        )
    };

    // POSIX security invariants:
    // 1. Effective must be a subset of permitted
    if (new_eff & !new_perm) != 0 {
        return Err(Errno::EPERM);
    }

    // 2. Unprivileged process cannot raise permitted capabilities beyond its current permitted set
    let current_perm = caller_proc.inner.lock().cap_permitted;
    let is_root = caller_proc.cred().cr_uid.as_u32() == 0;
    if !is_root && (new_perm & !current_perm) != 0 {
        return Err(Errno::EPERM);
    }

    {
        let mut inner = caller_proc.inner.lock();
        inner.cap_effective = new_eff;
        inner.cap_permitted = new_perm;
        inner.cap_inheritable = new_inh;
    }

    Ok(0)
}
