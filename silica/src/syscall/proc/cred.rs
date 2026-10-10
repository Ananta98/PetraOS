// SPDX-License-Identifier: GPL-2.0

//! Credential system calls.
//!
//! Handles UID, GID, supplementary groups, and filesystem identity queries and
//! transitions. Credentials are held in an immutable copy-on-write `Arc<Ucred>`.

use alloc::{sync::Arc, vec::Vec};
use ostd::arch::cpu::context::UserContext;

use crate::{
    api::{
        cred::{Gid, Uid},
        errno::{Errno, Result},
    },
    proc::thread::Thread,
    utils::userspace::user_ptr::UserPtr,
};

const NO_CHANGE: u32 = u32::MAX;
const NGROUPS_MAX: usize = 65536;

/// `getpid(2)` - returns process ID.
pub fn sys_getpid(_ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    Ok(proc.pid.as_u32() as usize)
}

/// `getppid(2)` - returns parent process ID.
pub fn sys_getppid(_ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let ppid = proc.inner.lock().ppid.map(|p| p.as_u32()).unwrap_or(0);
    Ok(ppid as usize)
}

/// `getuid(2)` - returns real user ID.
pub fn sys_getuid(_ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    Ok(proc.cred().cr_ruid.as_u32() as usize)
}

/// `getgid(2)` - returns real group ID.
pub fn sys_getgid(_ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    Ok(proc.cred().cr_rgid.as_u32() as usize)
}

/// `gettid(2)` - returns caller's thread ID (TID).
pub fn sys_gettid(_ctx: &mut UserContext) -> Result<usize> {
    let td = Thread::current().ok_or(Errno::ESRCH)?;
    Ok(td.tid.as_u32() as usize)
}

/// `geteuid(2)` - returns effective user ID.
pub fn sys_geteuid(_ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    Ok(proc.cred().cr_uid.as_u32() as usize)
}

/// `getegid(2)` - returns effective group ID.
pub fn sys_getegid(_ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    Ok(proc.cred().cr_gid.as_u32() as usize)
}

/// `setuid(2)` - sets user ID per POSIX rules.
pub fn sys_setuid(uid: u32, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let old_cred = proc.cred();

    let mut new_cred = (*old_cred).clone();
    if old_cred.is_superuser() {
        new_cred.cr_ruid = Uid(uid);
        new_cred.cr_uid = Uid(uid);
        new_cred.cr_svuid = Uid(uid);
        new_cred.cr_fsuid = Uid(uid);
    } else if uid == old_cred.cr_ruid.as_u32() || uid == old_cred.cr_svuid.as_u32() {
        new_cred.cr_uid = Uid(uid);
        new_cred.cr_fsuid = Uid(uid);
    } else {
        return Err(Errno::EPERM);
    }

    proc.set_cred(Arc::new(new_cred));
    Ok(0)
}

/// `setgid(2)` - sets group ID per POSIX rules.
pub fn sys_setgid(gid: u32, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let old_cred = proc.cred();

    let mut new_cred = (*old_cred).clone();
    if old_cred.is_superuser() {
        new_cred.cr_rgid = Gid(gid);
        new_cred.cr_gid = Gid(gid);
        new_cred.cr_svgid = Gid(gid);
        new_cred.cr_fsgid = Gid(gid);
    } else if gid == old_cred.cr_rgid.as_u32() || gid == old_cred.cr_svgid.as_u32() {
        new_cred.cr_gid = Gid(gid);
        new_cred.cr_fsgid = Gid(gid);
    } else {
        return Err(Errno::EPERM);
    }

    proc.set_cred(Arc::new(new_cred));
    Ok(0)
}

/// `setreuid(2)` - sets real and/or effective user ID.
pub fn sys_setreuid(ruid: u32, euid: u32, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let old_cred = proc.cred();

    let mut new_cred = (*old_cred).clone();
    let is_root = old_cred.is_superuser();

    if ruid != NO_CHANGE {
        if is_root || ruid == old_cred.cr_ruid.as_u32() || ruid == old_cred.cr_uid.as_u32() {
            new_cred.cr_ruid = Uid(ruid);
        } else {
            return Err(Errno::EPERM);
        }
    }

    if euid != NO_CHANGE {
        if is_root
            || euid == old_cred.cr_ruid.as_u32()
            || euid == old_cred.cr_uid.as_u32()
            || euid == old_cred.cr_svuid.as_u32()
        {
            new_cred.cr_uid = Uid(euid);
            new_cred.cr_fsuid = Uid(euid);
        } else {
            return Err(Errno::EPERM);
        }
    }

    if (ruid != NO_CHANGE || (euid != NO_CHANGE && euid != old_cred.cr_ruid.as_u32()))
        && (is_root || euid != NO_CHANGE)
    {
        new_cred.cr_svuid = new_cred.cr_uid;
    }

    proc.set_cred(Arc::new(new_cred));
    Ok(0)
}

/// `setregid(2)` - sets real and/or effective group ID.
pub fn sys_setregid(rgid: u32, egid: u32, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let old_cred = proc.cred();

    let mut new_cred = (*old_cred).clone();
    let is_root = old_cred.is_superuser();

    if rgid != NO_CHANGE {
        if is_root || rgid == old_cred.cr_rgid.as_u32() || rgid == old_cred.cr_gid.as_u32() {
            new_cred.cr_rgid = Gid(rgid);
        } else {
            return Err(Errno::EPERM);
        }
    }

    if egid != NO_CHANGE {
        if is_root
            || egid == old_cred.cr_rgid.as_u32()
            || egid == old_cred.cr_gid.as_u32()
            || egid == old_cred.cr_svgid.as_u32()
        {
            new_cred.cr_gid = Gid(egid);
            new_cred.cr_fsgid = Gid(egid);
        } else {
            return Err(Errno::EPERM);
        }
    }

    if (rgid != NO_CHANGE || (egid != NO_CHANGE && egid != old_cred.cr_rgid.as_u32()))
        && (is_root || egid != NO_CHANGE)
    {
        new_cred.cr_svgid = new_cred.cr_gid;
    }

    proc.set_cred(Arc::new(new_cred));
    Ok(0)
}

/// `setresuid(2)` - sets real, effective, and saved user ID.
pub fn sys_setresuid(ruid: u32, euid: u32, suid: u32, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let old_cred = proc.cred();

    let is_root = old_cred.is_superuser();
    let is_allowed = |id: u32| {
        id == NO_CHANGE
            || is_root
            || id == old_cred.cr_ruid.as_u32()
            || id == old_cred.cr_uid.as_u32()
            || id == old_cred.cr_svuid.as_u32()
    };

    if !is_allowed(ruid) || !is_allowed(euid) || !is_allowed(suid) {
        return Err(Errno::EPERM);
    }

    let mut new_cred = (*old_cred).clone();
    if ruid != NO_CHANGE {
        new_cred.cr_ruid = Uid(ruid);
    }
    if euid != NO_CHANGE {
        new_cred.cr_uid = Uid(euid);
        new_cred.cr_fsuid = Uid(euid);
    }
    if suid != NO_CHANGE {
        new_cred.cr_svuid = Uid(suid);
    }

    proc.set_cred(Arc::new(new_cred));
    Ok(0)
}

/// `getresuid(2)` - gets real, effective, and saved user ID.
pub fn sys_getresuid(
    ruid_ptr: usize,
    euid_ptr: usize,
    suid_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let cred = proc.cred();
    let vmar = proc.vmspace();

    let write_id = |ptr: usize, val: u32| -> Result<()> {
        if ptr == 0 {
            return Ok(());
        }
        UserPtr::new(ptr).write_bytes(&vmar, &val.to_ne_bytes())
    };

    write_id(ruid_ptr, cred.cr_ruid.as_u32())?;
    write_id(euid_ptr, cred.cr_uid.as_u32())?;
    write_id(suid_ptr, cred.cr_svuid.as_u32())?;

    Ok(0)
}

/// `setresgid(2)` - sets real, effective, and saved group ID.
pub fn sys_setresgid(rgid: u32, egid: u32, sgid: u32, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let old_cred = proc.cred();

    let is_root = old_cred.is_superuser();
    let is_allowed = |id: u32| {
        id == NO_CHANGE
            || is_root
            || id == old_cred.cr_rgid.as_u32()
            || id == old_cred.cr_gid.as_u32()
            || id == old_cred.cr_svgid.as_u32()
    };

    if !is_allowed(rgid) || !is_allowed(egid) || !is_allowed(sgid) {
        return Err(Errno::EPERM);
    }

    let mut new_cred = (*old_cred).clone();
    if rgid != NO_CHANGE {
        new_cred.cr_rgid = Gid(rgid);
    }
    if egid != NO_CHANGE {
        new_cred.cr_gid = Gid(egid);
        new_cred.cr_fsgid = Gid(egid);
    }
    if sgid != NO_CHANGE {
        new_cred.cr_svgid = Gid(sgid);
    }

    proc.set_cred(Arc::new(new_cred));
    Ok(0)
}

/// `getresgid(2)` - gets real, effective, and saved group ID.
pub fn sys_getresgid(
    rgid_ptr: usize,
    egid_ptr: usize,
    sgid_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let cred = proc.cred();
    let vmar = proc.vmspace();

    let write_id = |ptr: usize, val: u32| -> Result<()> {
        if ptr == 0 {
            return Ok(());
        }
        UserPtr::new(ptr).write_bytes(&vmar, &val.to_ne_bytes())
    };

    write_id(rgid_ptr, cred.cr_rgid.as_u32())?;
    write_id(egid_ptr, cred.cr_gid.as_u32())?;
    write_id(sgid_ptr, cred.cr_svgid.as_u32())?;

    Ok(0)
}

/// `getgroups(2)` - gets list of supplementary group IDs.
pub fn sys_getgroups(size: i32, list_ptr: usize, _ctx: &mut UserContext) -> Result<usize> {
    if size < 0 {
        return Err(Errno::EINVAL);
    }

    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let cred = proc.cred();
    let num_groups = cred.cr_groups.len();

    if size == 0 {
        return Ok(num_groups);
    }

    if (size as usize) < num_groups {
        return Err(Errno::EINVAL);
    }

    let vmar = proc.vmspace();
    let mut raw_bytes = Vec::with_capacity(num_groups * 4);
    for g in &cred.cr_groups {
        raw_bytes.extend_from_slice(&g.as_u32().to_ne_bytes());
    }

    UserPtr::new(list_ptr).write_bytes(&vmar, &raw_bytes)?;

    Ok(num_groups)
}

/// `setgroups(2)` - sets list of supplementary group IDs.
pub fn sys_setgroups(size: usize, list_ptr: usize, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let old_cred = proc.cred();

    if !old_cred.is_superuser() {
        return Err(Errno::EPERM);
    }

    if size > NGROUPS_MAX {
        return Err(Errno::EINVAL);
    }

    let mut new_groups = Vec::with_capacity(size);
    if size > 0 {
        let vmar = proc.vmspace();
        let byte_len = size * 4;
        let mut raw_bytes = alloc::vec![0u8; byte_len];

        UserPtr::new(list_ptr).read_bytes(&vmar, &mut raw_bytes)?;

        for chunk in raw_bytes.chunks_exact(4) {
            let gid = u32::from_ne_bytes(chunk.try_into().unwrap());
            new_groups.push(Gid(gid));
        }
    }

    let mut new_cred = (*old_cred).clone();
    new_cred.cr_groups = new_groups;
    proc.set_cred(Arc::new(new_cred));

    Ok(0)
}

/// `setfsuid(2)` - sets filesystem user ID. Returns previous fsuid.
pub fn sys_setfsuid(fsuid: u32, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let old_cred = proc.cred();
    let prev = old_cred.cr_fsuid.as_u32() as usize;

    if old_cred.is_superuser()
        || fsuid == old_cred.cr_ruid.as_u32()
        || fsuid == old_cred.cr_uid.as_u32()
        || fsuid == old_cred.cr_svuid.as_u32()
        || fsuid == old_cred.cr_fsuid.as_u32()
    {
        let mut new_cred = (*old_cred).clone();
        new_cred.cr_fsuid = Uid(fsuid);
        proc.set_cred(Arc::new(new_cred));
    }

    Ok(prev)
}

/// `setfsgid(2)` - sets filesystem group ID. Returns previous fsgid.
pub fn sys_setfsgid(fsgid: u32, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let old_cred = proc.cred();
    let prev = old_cred.cr_fsgid.as_u32() as usize;

    if old_cred.is_superuser()
        || fsgid == old_cred.cr_rgid.as_u32()
        || fsgid == old_cred.cr_gid.as_u32()
        || fsgid == old_cred.cr_svgid.as_u32()
        || fsgid == old_cred.cr_fsgid.as_u32()
    {
        let mut new_cred = (*old_cred).clone();
        new_cred.cr_fsgid = Gid(fsgid);
        proc.set_cred(Arc::new(new_cred));
    }

    Ok(prev)
}
