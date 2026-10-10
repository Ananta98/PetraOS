// SPDX-License-Identifier: GPL-2.0

//! System V Shared Memory segment representation.

use alloc::sync::Arc;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};

use ostd::sync::SpinLock;

use crate::{
    api::{ipc::IpcPerm, shm::ShmIdDs},
    proc::tree::Pid,
    vm::shm::SharedPages,
};

/// A System V shared memory segment.
pub struct ShmSegment {
    /// Segment identifier (`shmid`).
    pub id: i32,
    /// Associated IPC key (`IPC_PRIVATE` or user key).
    pub key: i32,
    /// Ownership and permissions.
    pub perm: SpinLock<IpcPerm>,
    /// Segment size in bytes.
    pub size: usize,
    /// Memory backing object (materialized page frames).
    pub pages: Arc<SharedPages>,
    /// Creator PID.
    pub cpid: Pid,
    /// Last operator PID (`shmat` / `shmdt`).
    pub lpid: AtomicU32,
    /// Number of active process attachments.
    pub nattch: AtomicUsize,
    /// Timestamp of last attach (`atime`).
    pub atime: AtomicU64,
    /// Timestamp of last detach (`dtime`).
    pub dtime: AtomicU64,
    /// Timestamp of last status change (`ctime`).
    pub ctime: AtomicU64,
    /// Marked for deletion via `IPC_RMID`.
    pub marked_for_deletion: AtomicBool,
}

impl ShmSegment {
    /// Constructs a new shared memory segment.
    pub fn new(id: i32, key: i32, perm: IpcPerm, size: usize, pages: Arc<SharedPages>, cpid: Pid, now: u64) -> Arc<Self> {
        Arc::new(Self {
            id,
            key,
            perm: SpinLock::new(perm),
            size,
            pages,
            cpid,
            lpid: AtomicU32::new(0),
            nattch: AtomicUsize::new(0),
            atime: AtomicU64::new(0),
            dtime: AtomicU64::new(0),
            ctime: AtomicU64::new(now),
            marked_for_deletion: AtomicBool::new(false),
        })
    }

    /// Exports current metadata into Linux `ShmIdDs`.
    pub fn to_shmid_ds(&self) -> ShmIdDs {
        ShmIdDs {
            shm_perm: *self.perm.lock(),
            shm_segsz: self.size,
            shm_atime: self.atime.load(Ordering::Relaxed) as i64,
            shm_dtime: self.dtime.load(Ordering::Relaxed) as i64,
            shm_ctime: self.ctime.load(Ordering::Relaxed) as i64,
            shm_cpid: self.cpid.as_u32(),
            shm_lpid: self.lpid.load(Ordering::Relaxed),
            shm_nattch: self.nattch.load(Ordering::Relaxed),
            glibc_reserved4: 0,
            glibc_reserved5: 0,
        }
    }
}
