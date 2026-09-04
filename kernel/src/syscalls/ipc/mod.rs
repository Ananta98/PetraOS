//! System V IPC Semaphore & Shared Memory System Call Handlers
//!
//! Implements:
//! - `sys_shmget`    (SYS_SHMGET    = 29):  create/open shared memory segment
//! - `sys_shmat`     (SYS_SHMAT     = 30):  attach shared memory segment
//! - `sys_shmctl`    (SYS_SHMCTL    = 31):  control shared memory segment
//! - `sys_semget`    (SYS_SEMGET    = 64):  open/create a semaphore set
//! - `sys_semop`     (SYS_SEMOP     = 65):  perform semaphore operations
//! - `sys_semctl`    (SYS_SEMCTL    = 66):  control operations on a semaphore set
//! - `sys_shmdt`     (SYS_SHMDT     = 67):  detach shared memory segment
//! - `sys_semtimedop`(SYS_SEMTIMEDOP = 220): `semop` with timeout

use super::{SyscallError, UserPtr};
use crate::ipc::semaphore::SemaphoreManager;
use crate::ipc::semaphore::{
    SemBuf, SemError,
};
use crate::ipc::shm::ShmError;

// ── Modular syscall submodules ──────────────────────────────────────────
pub mod semget;
pub mod semop;
pub mod semtimedop;
pub mod semctl;
pub mod shmget;
pub mod shmat;
pub mod shmdt;
pub mod shmctl;

pub use semget::sys_semget;
pub use semop::sys_semop;
pub use semtimedop::sys_semtimedop;
pub use semctl::sys_semctl;
pub use shmget::sys_shmget;
pub use shmat::sys_shmat;
pub use shmdt::sys_shmdt;
pub use shmctl::sys_shmctl;


/// Userspace `struct timespec` (64-bit layout).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct TimeSpec {
    pub tv_sec: i64,
    pub tv_nsec: i64,
}

/// Userspace union `semun` is passed as a raw `u64` (a pointer or integer).
/// We interpret it based on the `cmd` argument.
type SemunRaw = u64;

// ── Error conversion ──────────────────────────────────────────────────────────

impl From<SemError> for SyscallError {
    fn from(e: SemError) -> Self {
        match e {
            SemError::InvalidArg => SyscallError::EINVAL,
            SemError::NotFound => SyscallError::ENOENT,
            SemError::PermDenied => SyscallError::EACCES,
            SemError::AlreadyExists => SyscallError::EEXIST,
            SemError::Overflow => SyscallError::ERANGE,
            SemError::OutOfIds => SyscallError::ENOMEM,
            SemError::WouldBlock => SyscallError::EAGAIN,
            SemError::Removed => SyscallError::EIDRM,
            SemError::NoMem => SyscallError::ENOMEM,
        }
    }
}

impl From<ShmError> for SyscallError {
    fn from(e: ShmError) -> Self {
        match e {
            ShmError::InvalidArg => SyscallError::EINVAL,
            ShmError::NotFound => SyscallError::ENOENT,
            ShmError::PermDenied => SyscallError::EACCES,
            ShmError::AlreadyExists => SyscallError::EEXIST,
            ShmError::OutOfIds => SyscallError::ENOMEM,
            ShmError::NoMem => SyscallError::ENOMEM,
            ShmError::InUse => SyscallError::EBUSY,
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Retrieve the current process uid/gid for IPC permission creation.
pub(crate) fn current_uid_gid() -> (u32, u32) {
    if let Some(proc_arc) = crate::proc::current_process() {
        let proc = proc_arc.lock();
        (proc.creds.uid, proc.creds.gid)
    } else {
        (0, 0)
    }
}

/// Retrieve the current process PID as u32 for `sempid`.
pub(crate) fn current_pid_u32() -> u32 {
    crate::proc::current_process()
        .map(|p| p.lock().pid.as_u64() as u32)
        .unwrap_or(0)
}

/// Read a slice of `SemBuf` from userspace.
///
/// # Safety
/// The caller must ensure `ptr` and `nsops` are valid (validated via `UserPtr`).
pub(crate) fn read_sembuf_slice(ptr: u64, nsops: usize) -> Result<alloc::vec::Vec<SemBuf>, SyscallError> {
    if nsops == 0 || nsops > 500 {
        return Err(SyscallError::EINVAL);
    }

    let mut ops = alloc::vec::Vec::with_capacity(nsops);
    for i in 0..nsops {
        let elem_addr = ptr
            .checked_add((i * core::mem::size_of::<SemBuf>()) as u64)
            .ok_or(SyscallError::EFAULT)?;
        let uptr = UserPtr::<SemBuf>::from_u64(elem_addr);
        if !uptr.is_valid() {
            return Err(SyscallError::EFAULT);
        }
        let buf = uptr.read().ok_or(SyscallError::EFAULT)?;
        ops.push(buf);
    }
    Ok(ops)
}

// Re-export SemSet access for GETALL
impl SemaphoreManager {
    pub fn get_set_nsems(&self, semid: i32) -> Option<usize> {
        self.sets.get(&semid).map(|s| s.nsems())
    }
}

