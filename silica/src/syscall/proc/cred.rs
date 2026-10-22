// SPDX-License-Identifier: GPL-2.0

//! Credential system calls.
//!
//! Handles UID, GID, supplementary groups, and filesystem identity queries and
//! transitions. Credentials are held in an immutable copy-on-write `Arc<Ucred>`.

use alloc::{sync::Arc, vec::Vec};
use ostd::arch::cpu::context::UserContext;

use crate::{
    api::{
        cred::{Gid, Uid, Ucred},
        errno::{Errno, Result},
    },
    proc::thread::Thread,
    utils::userspace::UserPtr,
};

const NO_CHANGE: u32 = u32::MAX;
const NGROUPS_MAX: usize = 65536;

/// Wrapping behavior shared by `Uid`/`Gid` so the identically-ruled credential
/// transitions are written once for both families.
trait CredId: Copy {
    fn wrap(v: u32) -> Self;
    fn value(self) -> u32;
}

impl CredId for Uid {
    fn wrap(v: u32) -> Self {
        Uid(v)
    }
    fn value(self) -> u32 {
        self.0
    }
}

impl CredId for Gid {
    fn wrap(v: u32) -> Self {
        Gid(v)
    }
    fn value(self) -> u32 {
        self.0
    }
}

/// Accessor bundle pairing the uid or gid columns of `Ucred`.
///
/// `set_eff` covers `cr_uid`/`cr_fsuid` and `cr_gid`/`cr_fsgid` alike: in every
/// POSIX transition setting the effective ID also refreshes the filesystem ID.
struct IdColumns<Id> {
    real: fn(&Ucred) -> Id,
    eff: fn(&Ucred) -> Id,
    saved: fn(&Ucred) -> Id,
    fs: fn(&Ucred) -> Id,
    set_real: fn(&mut Ucred, Id),
    set_eff: fn(&mut Ucred, Id),
    set_fs: fn(&mut Ucred, Id),
    set_saved: fn(&mut Ucred, Id),
}

const UID_COLS: IdColumns<Uid> = IdColumns {
    real: |c| c.cr_ruid,
    eff: |c| c.cr_uid,
    saved: |c| c.cr_svuid,
    fs: |c| c.cr_fsuid,
    set_real: |c, v| c.cr_ruid = v,
    set_eff: |c, v| {
        c.cr_uid = v;
        c.cr_fsuid = v;
    },
    set_fs: |c, v| c.cr_fsuid = v,
    set_saved: |c, v| c.cr_svuid = v,
};

const GID_COLS: IdColumns<Gid> = IdColumns {
    real: |c| c.cr_rgid,
    eff: |c| c.cr_gid,
    saved: |c| c.cr_svgid,
    fs: |c| c.cr_fsgid,
    set_real: |c, v| c.cr_rgid = v,
    set_eff: |c, v| {
        c.cr_gid = v;
        c.cr_fsgid = v;
    },
    set_fs: |c, v| c.cr_fsgid = v,
    set_saved: |c, v| c.cr_svgid = v,
};

fn transition_single<Id: CredId>(
    proc: &Arc<crate::proc::Proc>,
    columns: &IdColumns<Id>,
    v: u32,
) -> Result<()> {
    let old_cred = proc.cred();
    let mut new_cred = (*old_cred).clone();
    if old_cred.is_superuser() {
        (columns.set_real)(&mut new_cred, Id::wrap(v));
        (columns.set_eff)(&mut new_cred, Id::wrap(v));
        (columns.set_saved)(&mut new_cred, Id::wrap(v));
        (columns.set_fs)(&mut new_cred, Id::wrap(v));
    } else if v == (columns.real)(&old_cred).value() || v == (columns.saved)(&old_cred).value() {
        (columns.set_eff)(&mut new_cred, Id::wrap(v));
    } else {
        return Err(Errno::EPERM);
    }

    proc.set_cred(Arc::new(new_cred));
    Ok(())
}

fn transition_re<Id: CredId>(
    proc: &Arc<crate::proc::Proc>,
    columns: &IdColumns<Id>,
    r: u32,
    e: u32,
) -> Result<()> {
    let old_cred = proc.cred();
    let mut new_cred = (*old_cred).clone();
    let is_root = old_cred.is_superuser();

    if r != NO_CHANGE {
        if is_root
            || r == (columns.real)(&old_cred).value()
            || r == (columns.eff)(&old_cred).value()
        {
            (columns.set_real)(&mut new_cred, Id::wrap(r));
        } else {
            return Err(Errno::EPERM);
        }
    }

    if e != NO_CHANGE {
        if is_root
            || e == (columns.real)(&old_cred).value()
            || e == (columns.eff)(&old_cred).value()
            || e == (columns.saved)(&old_cred).value()
        {
            (columns.set_eff)(&mut new_cred, Id::wrap(e));
        } else {
            return Err(Errno::EPERM);
        }
    }

    if (r != NO_CHANGE || (e != NO_CHANGE && e != (columns.real)(&old_cred).value()))
        && (is_root || e != NO_CHANGE)
    {
        let saved = (columns.eff)(&new_cred);
        (columns.set_saved)(&mut new_cred, saved);
    }

    proc.set_cred(Arc::new(new_cred));
    Ok(())
}

fn transition_res<Id: CredId>(
    proc: &Arc<crate::proc::Proc>,
    columns: &IdColumns<Id>,
    r: u32,
    e: u32,
    s: u32,
) -> Result<()> {
    let old_cred = proc.cred();

    let is_root = old_cred.is_superuser();
    let is_allowed = |id: u32| {
        id == NO_CHANGE
            || is_root
            || id == (columns.real)(&old_cred).value()
            || id == (columns.eff)(&old_cred).value()
            || id == (columns.saved)(&old_cred).value()
    };

    if !is_allowed(r) || !is_allowed(e) || !is_allowed(s) {
        return Err(Errno::EPERM);
    }

    let mut new_cred = (*old_cred).clone();
    if r != NO_CHANGE {
        (columns.set_real)(&mut new_cred, Id::wrap(r));
    }
    if e != NO_CHANGE {
        (columns.set_eff)(&mut new_cred, Id::wrap(e));
    }
    if s != NO_CHANGE {
        (columns.set_saved)(&mut new_cred, Id::wrap(s));
    }

    proc.set_cred(Arc::new(new_cred));
    Ok(())
}

fn report_res<Id: CredId>(
    proc: &Arc<crate::proc::Proc>,
    columns: &IdColumns<Id>,
    r_ptr: usize,
    e_ptr: usize,
    s_ptr: usize,
) -> Result<usize> {
    let cred = proc.cred();
    let vmar = proc.vmspace();

    UserPtr::new(r_ptr).write_opt(&vmar, (columns.real)(&cred).value())?;
    UserPtr::new(e_ptr).write_opt(&vmar, (columns.eff)(&cred).value())?;
    UserPtr::new(s_ptr).write_opt(&vmar, (columns.saved)(&cred).value())?;
    Ok(0)
}

fn transition_fs<Id: CredId>(
    proc: &Arc<crate::proc::Proc>,
    columns: &IdColumns<Id>,
    v: u32,
) -> Result<usize> {
    let old_cred = proc.cred();
    let prev = (columns.fs)(&old_cred).value() as usize;

    if old_cred.is_superuser()
        || v == (columns.real)(&old_cred).value()
        || v == (columns.eff)(&old_cred).value()
        || v == (columns.saved)(&old_cred).value()
        || v == (columns.fs)(&old_cred).value()
    {
        let mut new_cred = (*old_cred).clone();
        (columns.set_fs)(&mut new_cred, Id::wrap(v));
        proc.set_cred(Arc::new(new_cred));
    }

    Ok(prev)
}

/// `getpid(2)` - returns process ID.
pub fn sys_getpid(_ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    Ok(proc.pid.as_u32() as usize)
}

/// `getppid(2)` - returns parent process ID.
pub fn sys_getppid(_ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let ppid = proc.inner.lock().ppid.map(|p| p.as_u32()).unwrap_or(0);
    Ok(ppid as usize)
}

/// `getuid(2)` - returns real user ID.
pub fn sys_getuid(_ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    Ok(proc.cred().cr_ruid.as_u32() as usize)
}

/// `getgid(2)` - returns real group ID.
pub fn sys_getgid(_ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    Ok(proc.cred().cr_rgid.as_u32() as usize)
}

/// `gettid(2)` - returns caller's thread ID (TID).
pub fn sys_gettid(_ctx: &mut UserContext) -> Result<usize> {
    let td = Thread::current().ok_or(Errno::ESRCH)?;
    Ok(td.tid.as_u32() as usize)
}

/// `geteuid(2)` - returns effective user ID.
pub fn sys_geteuid(_ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    Ok(proc.cred().cr_uid.as_u32() as usize)
}

/// `getegid(2)` - returns effective group ID.
pub fn sys_getegid(_ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    Ok(proc.cred().cr_gid.as_u32() as usize)
}

/// `setuid(2)` - sets user ID per POSIX rules.
pub fn sys_setuid(uid: u32, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    transition_single(&proc, &UID_COLS, uid)?;
    Ok(0)
}

/// `setgid(2)` - sets group ID per POSIX rules.
pub fn sys_setgid(gid: u32, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    transition_single(&proc, &GID_COLS, gid)?;
    Ok(0)
}

/// `setreuid(2)` - sets real and/or effective user ID.
pub fn sys_setreuid(ruid: u32, euid: u32, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    transition_re(&proc, &UID_COLS, ruid, euid)?;
    Ok(0)
}

/// `setregid(2)` - sets real and/or effective group ID.
pub fn sys_setregid(rgid: u32, egid: u32, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    transition_re(&proc, &GID_COLS, rgid, egid)?;
    Ok(0)
}

/// `setresuid(2)` - sets real, effective, and saved user ID.
pub fn sys_setresuid(ruid: u32, euid: u32, suid: u32, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    transition_res(&proc, &UID_COLS, ruid, euid, suid)?;
    Ok(0)
}

/// `getresuid(2)` - gets real, effective, and saved user ID.
pub fn sys_getresuid(
    ruid_ptr: usize,
    euid_ptr: usize,
    suid_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    report_res(&proc, &UID_COLS, ruid_ptr, euid_ptr, suid_ptr)
}

/// `setresgid(2)` - sets real, effective, and saved group ID.
pub fn sys_setresgid(rgid: u32, egid: u32, sgid: u32, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    transition_res(&proc, &GID_COLS, rgid, egid, sgid)?;
    Ok(0)
}

/// `getresgid(2)` - gets real, effective, and saved group ID.
pub fn sys_getresgid(
    rgid_ptr: usize,
    egid_ptr: usize,
    sgid_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    report_res(&proc, &GID_COLS, rgid_ptr, egid_ptr, sgid_ptr)
}

/// `getgroups(2)` - gets list of supplementary group IDs.
pub fn sys_getgroups(size: i32, list_ptr: usize, _ctx: &mut UserContext) -> Result<usize> {
    if size < 0 {
        return Err(Errno::EINVAL);
    }

    let proc = Thread::proc_or_esrch()?;
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
    let proc = Thread::proc_or_esrch()?;
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
    let proc = Thread::proc_or_esrch()?;
    transition_fs(&proc, &UID_COLS, fsuid)
}

/// `setfsgid(2)` - sets filesystem group ID. Returns previous fsgid.
pub fn sys_setfsgid(fsgid: u32, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    transition_fs(&proc, &GID_COLS, fsgid)
}
