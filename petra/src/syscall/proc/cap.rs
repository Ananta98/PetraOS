// SPDX-License-Identifier: GPL-2.0

//! Capability system calls (`capget` #125, `capset` #126).

use ostd::arch::cpu::context::UserContext;

use crate::{
    proc::thread::Thread,
    uapi::errno::{Errno, Result},
    utils::UserPtr,
};

/// Linux capability ABI versions.
pub const LINUX_CAPABILITY_VERSION_1: u32 = 0x1998_0330;
pub const LINUX_CAPABILITY_VERSION_2: u32 = 0x2007_1026;
pub const LINUX_CAPABILITY_VERSION_3: u32 = 0x2008_0522;

/// System call `capget(cap_user_header_t hdrp, cap_user_data_t datap)` (Syscall #125).
pub fn sys_capget(hdrp: usize, datap: usize, _ctx: &mut UserContext) -> Result<usize> {
    if hdrp == 0 {
        return Err(Errno::EFAULT);
    }

    let caller_td = Thread::current().ok_or(Errno::ESRCH)?;
    let caller_proc = caller_td.proc().ok_or(Errno::ESRCH)?;
    let vmar = caller_proc.vmspace();

    let mut hdr_bytes = [0u8; 8];
    UserPtr::new(hdrp).read_bytes(&vmar, &mut hdr_bytes)?;
    let version = u32::from_ne_bytes(hdr_bytes[0..4].try_into().unwrap());
    let pid = i32::from_ne_bytes(hdr_bytes[4..8].try_into().unwrap());

    // Standard libcap version probe: datap == NULL writes preferred version and returns 0.
    if datap == 0 {
        let preferred = LINUX_CAPABILITY_VERSION_3.to_ne_bytes();
        UserPtr::new(hdrp).write_bytes(&vmar, &preferred)?;
        return Ok(0);
    }

    if version != LINUX_CAPABILITY_VERSION_1
        && version != LINUX_CAPABILITY_VERSION_2
        && version != LINUX_CAPABILITY_VERSION_3
    {
        let preferred = LINUX_CAPABILITY_VERSION_3.to_ne_bytes();
        UserPtr::new(hdrp).write_bytes(&vmar, &preferred)?;
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
        let mut data_bytes = [0u8; 12];
        data_bytes[0..4].copy_from_slice(&(eff as u32).to_ne_bytes());
        data_bytes[4..8].copy_from_slice(&(perm as u32).to_ne_bytes());
        data_bytes[8..12].copy_from_slice(&(inh as u32).to_ne_bytes());
        UserPtr::new(datap).write_bytes(&vmar, &data_bytes)?;
    } else {
        let mut data_bytes = [0u8; 24];
        data_bytes[0..4].copy_from_slice(&((eff & 0xffff_ffff) as u32).to_ne_bytes());
        data_bytes[4..8].copy_from_slice(&((perm & 0xffff_ffff) as u32).to_ne_bytes());
        data_bytes[8..12].copy_from_slice(&((inh & 0xffff_ffff) as u32).to_ne_bytes());

        data_bytes[12..16].copy_from_slice(&(((eff >> 32) & 0xffff_ffff) as u32).to_ne_bytes());
        data_bytes[16..20].copy_from_slice(&(((perm >> 32) & 0xffff_ffff) as u32).to_ne_bytes());
        data_bytes[20..24].copy_from_slice(&(((inh >> 32) & 0xffff_ffff) as u32).to_ne_bytes());
        UserPtr::new(datap).write_bytes(&vmar, &data_bytes)?;
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

    let mut hdr_bytes = [0u8; 8];
    UserPtr::new(hdrp).read_bytes(&vmar, &mut hdr_bytes)?;
    let version = u32::from_ne_bytes(hdr_bytes[0..4].try_into().unwrap());
    let pid = i32::from_ne_bytes(hdr_bytes[4..8].try_into().unwrap());

    if version != LINUX_CAPABILITY_VERSION_1
        && version != LINUX_CAPABILITY_VERSION_2
        && version != LINUX_CAPABILITY_VERSION_3
    {
        let preferred = LINUX_CAPABILITY_VERSION_3.to_ne_bytes();
        UserPtr::new(hdrp).write_bytes(&vmar, &preferred)?;
        return Err(Errno::EINVAL);
    }

    // In Linux, capset can only affect calling thread/process
    if pid != 0 && pid != caller_proc.pid.as_u32() as i32 {
        return Err(Errno::EPERM);
    }

    let (new_eff, new_perm, new_inh) = if version == LINUX_CAPABILITY_VERSION_1 {
        let mut data_bytes = [0u8; 12];
        UserPtr::new(datap).read_bytes(&vmar, &mut data_bytes)?;
        let eff = u32::from_ne_bytes(data_bytes[0..4].try_into().unwrap()) as u64;
        let perm = u32::from_ne_bytes(data_bytes[4..8].try_into().unwrap()) as u64;
        let inh = u32::from_ne_bytes(data_bytes[8..12].try_into().unwrap()) as u64;
        (eff, perm, inh)
    } else {
        let mut data_bytes = [0u8; 24];
        UserPtr::new(datap).read_bytes(&vmar, &mut data_bytes)?;
        let eff0 = u32::from_ne_bytes(data_bytes[0..4].try_into().unwrap()) as u64;
        let perm0 = u32::from_ne_bytes(data_bytes[4..8].try_into().unwrap()) as u64;
        let inh0 = u32::from_ne_bytes(data_bytes[8..12].try_into().unwrap()) as u64;

        let eff1 = u32::from_ne_bytes(data_bytes[12..16].try_into().unwrap()) as u64;
        let perm1 = u32::from_ne_bytes(data_bytes[16..20].try_into().unwrap()) as u64;
        let inh1 = u32::from_ne_bytes(data_bytes[20..24].try_into().unwrap()) as u64;

        (eff0 | (eff1 << 32), perm0 | (perm1 << 32), inh0 | (inh1 << 32))
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
