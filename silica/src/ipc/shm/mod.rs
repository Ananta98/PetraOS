// SPDX-License-Identifier: GPL-2.0

//! System V Shared Memory implementation.
//!
//! Provides `shmget`, `shmat`, `shmctl`, and `shmdt` semantics following POSIX and Linux x86-64.

pub mod segment;

use alloc::{
    collections::BTreeMap,
    sync::Arc,
};
use core::sync::atomic::{AtomicI32, Ordering};
use ostd::{
    mm::PAGE_SIZE,
    sync::SpinLock,
};

use crate::{
    api::{
        cred::Ucred,
        errno::{Errno, Result},
        ipc::{IPC_CREAT, IPC_EXCL, IPC_INFO, IPC_PRIVATE, IPC_RMID, IPC_SET, IPC_STAT, IpcPerm},
        shm::{
            SHMMAX, SHMMIN, SHMMNI, SHM_EXEC, SHM_INFO, SHM_LOCK, SHM_RDONLY, SHM_REMAP,
            SHM_RND, SHM_STAT, SHM_STAT_ANY, SHM_UNLOCK, SHMLBA, ShmIdDs, ShmInfo,
        },
    },
    ipc::perm::{check_admin_permission, check_read_permission, check_write_permission},
    proc::{tree::Pid, Proc},
    utils::userspace::user_ptr::UserPtr,
    vm::{
        flags::MmapFlags,
        perms::VmPerms,
        shm::SharedPages,
        vmar::{VMAR_CAP_ADDR, VMAR_LOWEST_ADDR},
        backing::Backing,
    },
};

pub use self::segment::ShmSegment;

/// A per-process shared memory attachment record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShmAttachment {
    pub shmid: i32,
    pub addr: usize,
    pub size: usize,
    pub readonly: bool,
}

static SHM_REGISTRY: SpinLock<BTreeMap<i32, Arc<ShmSegment>>> = SpinLock::new(BTreeMap::new());
static KEY_MAP: SpinLock<BTreeMap<i32, i32>> = SpinLock::new(BTreeMap::new());
static NEXT_SHMID: AtomicI32 = AtomicI32::new(1);

fn now_sec() -> u64 {
    crate::sched::now() / crate::sched::TIMER_HZ
}

fn alloc_shmid() -> i32 {
    NEXT_SHMID.fetch_add(1, Ordering::Relaxed)
}

/// System call implementation for `shmget(2)`.
pub fn shm_get(key: i32, size: usize, shmflg: i32, cred: &Ucred, pid: Pid) -> Result<i32> {
    if key == IPC_PRIVATE {
        if size < SHMMIN || size > SHMMAX {
            crate::return_errno!(EINVAL, "shared memory size out of bounds");
        }
        let aligned_size = size.checked_next_multiple_of(PAGE_SIZE).ok_or(Errno::EINVAL)?;
        let shmid = alloc_shmid();
        let perm = IpcPerm::new(key, cred, (shmflg as u32) & 0o777);
        let pages = SharedPages::new(aligned_size)?;
        let segment = ShmSegment::new(shmid, key, perm, aligned_size, pages, pid, now_sec());

        let mut reg = SHM_REGISTRY.lock();
        if reg.len() >= SHMMNI {
            crate::return_errno!(ENOSPC, "maximum number of shared memory segments reached");
        }
        reg.insert(shmid, segment);
        return Ok(shmid);
    }

    let mut key_map = KEY_MAP.lock();
    if let Some(&shmid) = key_map.get(&key) {
        if (shmflg & IPC_CREAT) != 0 && (shmflg & IPC_EXCL) != 0 {
            crate::return_errno!(EEXIST, "IPC segment already exists");
        }

        let reg = SHM_REGISTRY.lock();
        let segment = reg.get(&shmid).ok_or(Errno::ENOENT)?;

        if size > segment.size {
            crate::return_errno!(EINVAL, "requested size exceeds existing segment size");
        }

        let perm = segment.perm.lock();
        let req_mode = (shmflg as u32) & 0o777;
        if !perm.check_perm(cred, req_mode) {
            crate::return_errno!(EACCES, "permission denied for existing segment");
        }

        return Ok(shmid);
    }

    // Key not found
    if (shmflg & IPC_CREAT) == 0 {
        crate::return_errno!(ENOENT, "IPC segment does not exist");
    }

    if size < SHMMIN || size > SHMMAX {
        crate::return_errno!(EINVAL, "shared memory size out of bounds");
    }

    let aligned_size = size.checked_next_multiple_of(PAGE_SIZE).ok_or(Errno::EINVAL)?;
    let shmid = alloc_shmid();
    let perm = IpcPerm::new(key, cred, (shmflg as u32) & 0o777);
    let pages = SharedPages::new(aligned_size)?;
    let segment = ShmSegment::new(shmid, key, perm, aligned_size, pages, pid, now_sec());

    let mut reg = SHM_REGISTRY.lock();
    if reg.len() >= SHMMNI {
        crate::return_errno!(ENOSPC, "maximum number of shared memory segments reached");
    }

    key_map.insert(key, shmid);
    reg.insert(shmid, segment);

    Ok(shmid)
}

/// System call implementation for `shmat(2)`.
pub fn shm_at(shmid: i32, shmaddr: usize, shmflg: i32, cred: &Ucred, proc: &Arc<Proc>) -> Result<usize> {
    let segment = {
        let reg = SHM_REGISTRY.lock();
        reg.get(&shmid).cloned().ok_or(Errno::EINVAL)?
    };

    // Check permissions
    {
        let perm = segment.perm.lock();
        check_read_permission(&perm, cred)?;
        if (shmflg & SHM_RDONLY) == 0 {
            check_write_permission(&perm, cred)?;
        }
    }

    // Calculate attach address
    let (target_addr, fixed) = if shmaddr == 0 {
        (0, false)
    } else if (shmflg & SHM_RND) != 0 {
        let rounded = shmaddr.saturating_sub(shmaddr % SHMLBA);
        (rounded, true)
    } else {
        if !shmaddr.is_multiple_of(PAGE_SIZE) {
            crate::return_errno!(EINVAL, "attach address not page-aligned");
        }
        (shmaddr, true)
    };

    if fixed && (target_addr < VMAR_LOWEST_ADDR || target_addr >= VMAR_CAP_ADDR) {
        crate::return_errno!(EINVAL, "attach address out of user address bounds");
    }

    let mut perms = VmPerms::READ;
    if (shmflg & SHM_RDONLY) == 0 {
        perms |= VmPerms::WRITE;
    }
    if (shmflg & SHM_EXEC) != 0 {
        perms |= VmPerms::EXEC;
    }

    let mut mmap_flags = MmapFlags::SHARED;
    if fixed {
        if (shmflg & SHM_REMAP) != 0 {
            mmap_flags |= MmapFlags::FIXED;
        } else {
            mmap_flags |= MmapFlags::FIXED_NOREPLACE;
        }
    }

    let backing = Arc::clone(&segment.pages) as Arc<dyn Backing>;
    let vmar = proc.vmspace();
    let mapped_addr = vmar.mmap_backed(&backing, target_addr, segment.size, perms, mmap_flags, 0)?;

    // Record attachment in process descriptor
    {
        let mut inner = proc.inner.lock();
        inner.shm_attachments.insert(
            mapped_addr,
            ShmAttachment {
                shmid,
                addr: mapped_addr,
                size: segment.size,
                readonly: (shmflg & SHM_RDONLY) != 0,
            },
        );
    }

    // Update segment statistics
    segment.nattch.fetch_add(1, Ordering::Relaxed);
    segment.atime.store(now_sec(), Ordering::Relaxed);
    segment.lpid.store(proc.pid.as_u32(), Ordering::Relaxed);

    Ok(mapped_addr)
}

/// System call implementation for `shmdt(2)`.
pub fn shm_dt(shmaddr: usize, proc: &Arc<Proc>) -> Result<()> {
    if !shmaddr.is_multiple_of(PAGE_SIZE) {
        crate::return_errno!(EINVAL, "detach address not page aligned");
    }

    let attachment = {
        let mut inner = proc.inner.lock();
        inner.shm_attachments.remove(&shmaddr).ok_or(Errno::EINVAL)?
    };

    let vmar = proc.vmspace();
    vmar.munmap(attachment.addr..(attachment.addr + attachment.size))?;

    // Update segment statistics
    let segment = {
        let reg = SHM_REGISTRY.lock();
        reg.get(&attachment.shmid).cloned()
    };

    if let Some(segment) = segment {
        let prev = segment.nattch.fetch_sub(1, Ordering::Relaxed);
        segment.dtime.store(now_sec(), Ordering::Relaxed);
        segment.lpid.store(proc.pid.as_u32(), Ordering::Relaxed);

        if prev <= 1 && segment.marked_for_deletion.load(Ordering::Relaxed) {
            // Reap deleted segment
            let mut reg = SHM_REGISTRY.lock();
            let mut key_map = KEY_MAP.lock();
            reg.remove(&attachment.shmid);
            if segment.key != IPC_PRIVATE {
                key_map.remove(&segment.key);
            }
        }
    }

    Ok(())
}

/// System call implementation for `shmctl(2)`.
pub fn shm_ctl(shmid: i32, cmd: i32, buf_ptr: usize, cred: &Ucred, proc: &Arc<Proc>) -> Result<usize> {
    let vmar = proc.vmspace();

    if cmd == IPC_INFO || cmd == SHM_INFO {
        if buf_ptr == 0 {
            return Err(Errno::EFAULT);
        }
        let info = ShmInfo::default();
        UserPtr::new(buf_ptr).write_bytes(&vmar, &info.to_bytes())?;
        return Ok(SHMMNI);
    }

    let segment = {
        let reg = SHM_REGISTRY.lock();
        reg.get(&shmid).cloned().ok_or(Errno::EINVAL)?
    };

    if cmd == IPC_STAT || cmd == SHM_STAT {
        if buf_ptr == 0 {
            return Err(Errno::EFAULT);
        }
        if cmd == IPC_STAT {
            let perm = segment.perm.lock();
            check_read_permission(&perm, cred)?;
        }
        let ds = segment.to_shmid_ds();
        UserPtr::new(buf_ptr).write_bytes(&vmar, &ds.to_bytes())?;
        return Ok(0);
    }

    if cmd == SHM_STAT_ANY {
        if buf_ptr == 0 {
            return Err(Errno::EFAULT);
        }
        let ds = segment.to_shmid_ds();
        UserPtr::new(buf_ptr).write_bytes(&vmar, &ds.to_bytes())?;
        return Ok(0);
    }

    if cmd == IPC_SET {
        if buf_ptr == 0 {
            return Err(Errno::EFAULT);
        }
        let mut perm = segment.perm.lock();
        check_admin_permission(&perm, cred)?;

        let mut raw = [0u8; 112];
        UserPtr::new(buf_ptr).read_bytes(&vmar, &mut raw)?;
        let new_ds = ShmIdDs::from_bytes(&raw).ok_or(Errno::EINVAL)?;

        perm.uid = new_ds.shm_perm.uid;
        perm.gid = new_ds.shm_perm.gid;
        perm.mode = (perm.mode & !0o777) | (new_ds.shm_perm.mode & 0o777);
        segment.ctime.store(now_sec(), Ordering::Relaxed);
        return Ok(0);
    }

    if cmd == IPC_RMID {
        let perm = segment.perm.lock();
        check_admin_permission(&perm, cred)?;

        segment.marked_for_deletion.store(true, Ordering::Relaxed);
        if segment.nattch.load(Ordering::Relaxed) == 0 {
            let mut reg = SHM_REGISTRY.lock();
            let mut key_map = KEY_MAP.lock();
            reg.remove(&shmid);
            if segment.key != IPC_PRIVATE {
                key_map.remove(&segment.key);
            }
        }
        return Ok(0);
    }

    if cmd == SHM_LOCK || cmd == SHM_UNLOCK {
        if !cred.cr_uid.is_root() {
            return Err(Errno::EPERM);
        }
        return Ok(0);
    }

    Err(Errno::EINVAL)
}

/// Detaches all shared memory segments when process terminates.
pub fn on_proc_exit(proc: &Proc) {
    let attachments = {
        let mut inner = proc.inner.lock();
        core::mem::take(&mut inner.shm_attachments)
    };

    if attachments.is_empty() {
        return;
    }

    let mut reg = SHM_REGISTRY.lock();
    let mut key_map = KEY_MAP.lock();

    for (_, att) in attachments {
        if let Some(segment) = reg.get(&att.shmid) {
            let prev = segment.nattch.fetch_sub(1, Ordering::Relaxed);
            segment.dtime.store(now_sec(), Ordering::Relaxed);
            segment.lpid.store(proc.pid.as_u32(), Ordering::Relaxed);

            if prev <= 1 && segment.marked_for_deletion.load(Ordering::Relaxed) {
                let key = segment.key;
                let id = segment.id;
                reg.remove(&id);
                if key != IPC_PRIVATE {
                    key_map.remove(&key);
                }
            }
        }
    }
}

/// Clones shared memory attachments from parent to child across `fork(2)`.
pub fn on_proc_fork(parent: &Proc, child: &Proc) {
    let parent_attachments = parent.inner.lock().shm_attachments.clone();
    if parent_attachments.is_empty() {
        return;
    }

    let reg = SHM_REGISTRY.lock();
    for (_, att) in &parent_attachments {
        if let Some(segment) = reg.get(&att.shmid) {
            segment.nattch.fetch_add(1, Ordering::Relaxed);
        }
    }

    child.inner.lock().shm_attachments = parent_attachments;
}
