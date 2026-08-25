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

use super::{SyscallError, SyscallResult, UserPtr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::arch::timer::hpet;
use crate::ipc::semaphore::SemaphoreManager;
use crate::ipc::semaphore::{
    GETALL, GETNCNT, GETPID, GETVAL, GETZCNT, IPC_RMID, IPC_SET, IPC_STAT,
    SEMAPHORE_MANAGER, SETALL, SETVAL, SemBuf, SemError, SemidDs, SemopResult,
};
use crate::ipc::shm::{SHM_MANAGER, ShmError, ShmidDs, ShmInfo};
use crate::proc::thread::ThreadState;

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
fn current_uid_gid() -> (u32, u32) {
    if let Some(proc_arc) = crate::proc::current_process() {
        let proc = proc_arc.lock();
        (proc.creds.uid, proc.creds.gid)
    } else {
        (0, 0)
    }
}

/// Retrieve the current process PID as u32 for `sempid`.
fn current_pid_u32() -> u32 {
    crate::proc::current_process()
        .map(|p| p.lock().pid.as_u64() as u32)
        .unwrap_or(0)
}

/// Read a slice of `SemBuf` from userspace.
///
/// # Safety
/// The caller must ensure `ptr` and `nsops` are valid (validated via `UserPtr`).
fn read_sembuf_slice(ptr: u64, nsops: usize) -> Result<alloc::vec::Vec<SemBuf>, SyscallError> {
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

// ── sys_semget ────────────────────────────────────────────────────────────────

/// `semget(key, nsems, semflg)` — Open or create a System V semaphore set.
///
/// - `arg1`: `key_t key`  (IPC key, or `IPC_PRIVATE` for anonymous)
/// - `arg2`: `int nsems`  (number of semaphores in the set)
/// - `arg3`: `int semflg` (creation flags + permission bits)
///
/// Returns semaphore set identifier on success, or negative errno.
pub fn sys_semget(frame: &mut SyscallFrame) -> SyscallResult {
    let key = frame.arg1() as i32;
    let nsems = frame.arg2() as i32;
    let semflg = frame.arg3() as i32;

    let (uid, gid) = current_uid_gid();

    let mut mgr = SEMAPHORE_MANAGER.lock();
    let semid = mgr.semget(key, nsems, semflg, uid, gid)?;
    Ok(semid as usize)
}

// ── sys_semop ─────────────────────────────────────────────────────────────────

/// `semop(semid, sops, nsops)` — Perform operations on a semaphore set.
///
/// - `arg1`: `int semid`
/// - `arg2`: `struct sembuf *sops`
/// - `arg3`: `size_t nsops`
///
/// Returns 0 on success, or negative errno.
pub fn sys_semop(frame: &mut SyscallFrame) -> SyscallResult {
    let semid = frame.arg1() as i32;
    let sops_ptr = frame.arg2();
    let nsops = frame.arg3() as usize;

    let ops = read_sembuf_slice(sops_ptr, nsops)?;
    let pid = current_pid_u32();

    let thread_arc = crate::proc::current_thread().ok_or(SyscallError::ESRCH)?;

    loop {
        let result = {
            let mut mgr = SEMAPHORE_MANAGER.lock();
            mgr.semop_try(semid, &ops, thread_arc.clone(), pid, false)
        };

        match result {
            Ok(SemopResult::Done) => return Ok(0),
            Ok(SemopResult::Block { .. }) => {
                // Put current thread to sleep
                {
                    let mut t = thread_arc.lock();
                    t.state = ThreadState::Sleeping;
                }
                crate::sched::schedule(false);

                // Woken up – retry or check for removal
                let retry = {
                    let mut mgr = SEMAPHORE_MANAGER.lock();
                    mgr.semop_retry(semid, &ops, pid)
                };

                match retry {
                    Ok(true) => return Err(SyscallError::EIDRM),
                    Ok(false) => return Ok(0),
                    Err(SemError::WouldBlock) => continue, // spurious wakeup – loop again
                    Err(e) => return Err(e.into()),
                }
            }
            Err(e) => return Err(e.into()),
        }
    }
}

// ── sys_semtimedop ────────────────────────────────────────────────────────────

/// `semtimedop(semid, sops, nsops, timeout)` — `semop` with absolute timeout.
///
/// - `arg1`: `int semid`
/// - `arg2`: `struct sembuf *sops`
/// - `arg3`: `size_t nsops`
/// - `arg4`: `const struct timespec *timeout` (relative; NULL = no timeout)
///
/// Returns 0 on success, or negative errno.
pub fn sys_semtimedop(frame: &mut SyscallFrame) -> SyscallResult {
    let semid = frame.arg1() as i32;
    let sops_ptr = frame.arg2();
    let nsops = frame.arg3() as usize;
    let timeout_ptr = frame.arg4();

    let ops = read_sembuf_slice(sops_ptr, nsops)?;
    let pid = current_pid_u32();

    // Parse optional timeout
    let deadline_ns: Option<u64> = if timeout_ptr != 0 {
        let ts_uptr = UserPtr::<TimeSpec>::from_u64(timeout_ptr);
        if !ts_uptr.is_valid() {
            return Err(SyscallError::EFAULT);
        }
        let ts = ts_uptr.read().ok_or(SyscallError::EFAULT)?;
        if ts.tv_sec < 0 || ts.tv_nsec < 0 || ts.tv_nsec >= 1_000_000_000 {
            return Err(SyscallError::EINVAL);
        }
        let dur_ns = (ts.tv_sec as u64)
            .saturating_mul(1_000_000_000)
            .saturating_add(ts.tv_nsec as u64);
        Some(hpet::elapsed_ns().saturating_add(dur_ns))
    } else {
        None
    };

    let thread_arc = crate::proc::current_thread().ok_or(SyscallError::ESRCH)?;

    loop {
        // Check deadline before attempting
        if let Some(dl) = deadline_ns {
            if hpet::elapsed_ns() >= dl {
                return Err(SyscallError::ETIMEDOUT);
            }
        }

        let result = {
            let mut mgr = SEMAPHORE_MANAGER.lock();
            mgr.semop_try(semid, &ops, thread_arc.clone(), pid, false)
        };

        match result {
            Ok(crate::ipc::semaphore::SemopResult::Done) => return Ok(0),
            Ok(crate::ipc::semaphore::SemopResult::Block { .. }) => {
                {
                    let mut t = thread_arc.lock();
                    t.state = ThreadState::Sleeping;
                }
                crate::sched::schedule(false);

                // Check timeout on wakeup
                if let Some(dl) = deadline_ns {
                    if crate::arch::timer::hpet::elapsed_ns() >= dl {
                        return Err(SyscallError::ETIMEDOUT);
                    }
                }

                let retry = {
                    let mut mgr = SEMAPHORE_MANAGER.lock();
                    mgr.semop_retry(semid, &ops, pid)
                };

                match retry {
                    Ok(true) => return Err(SyscallError::EIDRM),
                    Ok(false) => return Ok(0),
                    Err(SemError::WouldBlock) => continue,
                    Err(e) => return Err(e.into()),
                }
            }
            Err(e) => return Err(e.into()),
        }
    }
}

// ── sys_semctl ────────────────────────────────────────────────────────────────

/// `semctl(semid, semnum, cmd, arg)` — Control operations on a semaphore set.
///
/// - `arg1`: `int semid`
/// - `arg2`: `int semnum`
/// - `arg3`: `int cmd`
/// - `arg4`: `union semun` (raw value / pointer depending on cmd)
///
/// Returns ≥ 0 on success, or negative errno.
pub fn sys_semctl(frame: &mut SyscallFrame) -> SyscallResult {
    let semid = frame.arg1() as i32;
    let semnum = frame.arg2() as i32;
    let cmd = frame.arg3() as i32;
    let arg_raw: SemunRaw = frame.arg4();

    let (uid, _gid) = current_uid_gid();

    // Strip IPC_64 flag (value 0 on x86_64 – no-op)
    let cmd_stripped = cmd & !0x100;

    match cmd_stripped {
        // Commands that don't use union semun argument
        IPC_RMID => {
            let mut mgr = SEMAPHORE_MANAGER.lock();
            let result = mgr.semctl(semid, semnum, IPC_RMID, None, None, None, None, uid)?;
            Ok(result as usize)
        }

        GETVAL | GETPID | GETNCNT | GETZCNT => {
            let mut mgr = SEMAPHORE_MANAGER.lock();
            let result = mgr.semctl(semid, semnum, cmd_stripped, None, None, None, None, uid)?;
            Ok(result as usize)
        }

        SETVAL => {
            // arg_raw is an int value (not a pointer) on x86_64
            let val = arg_raw as i32;
            let mut mgr = SEMAPHORE_MANAGER.lock();
            let result = mgr.semctl(semid, semnum, SETVAL, Some(val), None, None, None, uid)?;
            Ok(result as usize)
        }

        IPC_STAT => {
            // arg_raw is a pointer to `struct semid_ds`
            let ds_uptr = UserPtr::<SemidDs>::from_u64(arg_raw);
            if !ds_uptr.is_valid() {
                return Err(SyscallError::EFAULT);
            }
            let mut ds = SemidDs::default();
            {
                let mut mgr = SEMAPHORE_MANAGER.lock();
                mgr.semctl(
                    semid,
                    semnum,
                    IPC_STAT,
                    None,
                    Some(&mut ds),
                    None,
                    None,
                    uid,
                )?;
            }
            // SAFETY: pointer is validated via UserPtr::is_valid()
            unsafe {
                let raw = ds_uptr.as_ptr() as *mut SemidDs;
                core::ptr::write_volatile(raw, ds);
            }
            Ok(0)
        }

        IPC_SET => {
            let ds_uptr = UserPtr::<SemidDs>::from_u64(arg_raw);
            if !ds_uptr.is_valid() {
                return Err(SyscallError::EFAULT);
            }
            let mut ds = ds_uptr.read().ok_or(SyscallError::EFAULT)?;
            let mut mgr = SEMAPHORE_MANAGER.lock();
            mgr.semctl(semid, semnum, IPC_SET, None, Some(&mut ds), None, None, uid)?;
            Ok(0)
        }

        GETALL => {
            // arg_raw is pointer to u16 array
            let set_nsems = {
                let mgr = SEMAPHORE_MANAGER.lock();
                mgr.sets.get(&semid).ok_or(SyscallError::ENOENT)?.nsems()
            };
            let mut buf: alloc::vec::Vec<u16> = alloc::vec![0u16; set_nsems];
            {
                let mut mgr = SEMAPHORE_MANAGER.lock();
                mgr.semctl(semid, semnum, GETALL, None, None, None, Some(&mut buf), uid)?;
            }
            // Write the array back to userspace
            for (i, &v) in buf.iter().enumerate() {
                let elem_addr = arg_raw
                    .checked_add((i * 2) as u64)
                    .ok_or(SyscallError::EFAULT)?;
                let uptr = UserPtr::<u16>::from_u64(elem_addr);
                if !uptr.is_valid() {
                    return Err(SyscallError::EFAULT);
                }
                // SAFETY: validated via UserPtr::is_valid()
                unsafe {
                    core::ptr::write_volatile(uptr.as_ptr() as *mut u16, v);
                }
            }
            Ok(0)
        }

        SETALL => {
            let set_nsems = {
                let mgr = SEMAPHORE_MANAGER.lock();
                mgr.sets.get(&semid).ok_or(SyscallError::ENOENT)?.nsems()
            };
            let mut buf: alloc::vec::Vec<u16> = alloc::vec![0u16; set_nsems];
            for (i, slot) in buf.iter_mut().enumerate() {
                let elem_addr = arg_raw
                    .checked_add((i * 2) as u64)
                    .ok_or(SyscallError::EFAULT)?;
                let uptr = UserPtr::<u16>::from_u64(elem_addr);
                if !uptr.is_valid() {
                    return Err(SyscallError::EFAULT);
                }
                *slot = uptr.read().ok_or(SyscallError::EFAULT)?;
            }
            let mut mgr = SEMAPHORE_MANAGER.lock();
            mgr.semctl(semid, semnum, SETALL, None, None, Some(&buf), None, uid)?;
            Ok(0)
        }

        _ => Err(SyscallError::EINVAL),
    }
}

// Re-export SemSet access for GETALL
impl SemaphoreManager {
    pub fn get_set_nsems(&self, semid: i32) -> Option<usize> {
        self.sets.get(&semid).map(|s| s.nsems())
    }
}

// ── sys_shmget ────────────────────────────────────────────────────────────────

/// `shmget(key, size, shmflg)` — Allocates a System V shared memory segment.
///
/// - `arg1`: `key_t key` (IPC key, or `IPC_PRIVATE` for anonymous)
/// - `arg2`: `size_t size` (segment size in bytes)
/// - `arg3`: `int shmflg` (creation flags + permission bits)
///
/// Returns shared memory identifier on success, or negative errno.
pub fn sys_shmget(frame: &mut SyscallFrame) -> SyscallResult {
    let key = frame.arg1() as i32;
    let size = frame.arg2() as usize;
    let shmflg = frame.arg3() as i32;

    let (uid, gid) = current_uid_gid();
    let pid = current_pid_u32();

    let mut mgr = SHM_MANAGER.lock();
    let shmid = mgr.shmget(key, size, shmflg, uid, gid, pid)?;
    Ok(shmid as usize)
}

// ── sys_shmat ─────────────────────────────────────────────────────────────────

/// `shmat(shmid, shmaddr, shmflg)` — Attaches the System V shared memory segment to the calling address space.
///
/// - `arg1`: `int shmid`
/// - `arg2`: `const void *shmaddr` (requested virtual address or 0 for automatic)
/// - `arg3`: `int shmflg` (attachment flags)
///
/// Returns attached virtual address on success, or negative errno.
pub fn sys_shmat(frame: &mut SyscallFrame) -> SyscallResult {
    let shmid = frame.arg1() as i32;
    let shmaddr = frame.arg2();
    let shmflg = frame.arg3() as i32;

    let (uid, gid) = current_uid_gid();
    let pid = current_pid_u32();

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let mut proc = proc_arc.lock();

    let addr_space_arc = alloc::sync::Arc::clone(&proc.address_space);
    let mut addr_space = addr_space_arc.lock();
    let mut mgr = SHM_MANAGER.lock();

    let vaddr = mgr.shmat(
        shmid,
        shmaddr,
        shmflg,
        uid,
        gid,
        pid,
        &mut addr_space,
        &mut proc.mmap_bump,
    )?;

    Ok(vaddr as usize)
}

// ── sys_shmdt ─────────────────────────────────────────────────────────────────

/// `shmdt(shmaddr)` — Detaches the System V shared memory segment from the calling address space.
///
/// - `arg1`: `const void *shmaddr`
///
/// Returns 0 on success, or negative errno.
pub fn sys_shmdt(frame: &mut SyscallFrame) -> SyscallResult {
    let shmaddr = frame.arg1();
    let pid = current_pid_u32();

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();

    let addr_space_arc = alloc::sync::Arc::clone(&proc.address_space);
    let mut addr_space = addr_space_arc.lock();
    let mut mgr = SHM_MANAGER.lock();

    mgr.shmdt(shmaddr, pid, &mut addr_space)?;

    Ok(0)
}

// ── sys_shmctl ────────────────────────────────────────────────────────────────

/// `shmctl(shmid, cmd, buf)` — System V shared memory control operations.
///
/// - `arg1`: `int shmid`
/// - `arg2`: `int cmd`
/// - `arg3`: `struct shmid_ds *buf`
///
/// Returns ≥ 0 on success, or negative errno.
pub fn sys_shmctl(frame: &mut SyscallFrame) -> SyscallResult {
    let shmid = frame.arg1() as i32;
    let cmd = frame.arg2() as i32;
    let buf_ptr = frame.arg3();

    let (uid, gid) = current_uid_gid();
    let cmd_stripped = cmd & !0x100; // Strip IPC_64 flag if present

    match cmd_stripped {
        crate::ipc::shm::IPC_RMID => {
            let mut mgr = SHM_MANAGER.lock();
            mgr.shmctl(shmid, crate::ipc::shm::IPC_RMID, None, None, None, uid, gid)?;
            Ok(0)
        }
        crate::ipc::shm::IPC_STAT | crate::ipc::shm::SHM_STAT => {
            let ds_uptr = UserPtr::<ShmidDs>::from_u64(buf_ptr);
            if !ds_uptr.is_valid() {
                return Err(SyscallError::EFAULT);
            }
            let mut ds = ShmidDs::default();
            let res = {
                let mut mgr = SHM_MANAGER.lock();
                mgr.shmctl(shmid, cmd_stripped, Some(&mut ds), None, None, uid, gid)?
            };
            ds_uptr.write(ds).ok_or(SyscallError::EFAULT)?;
            Ok(res as usize)
        }
        crate::ipc::shm::IPC_SET => {
            let ds_uptr = UserPtr::<ShmidDs>::from_u64(buf_ptr);
            if !ds_uptr.is_valid() {
                return Err(SyscallError::EFAULT);
            }
            let ds = ds_uptr.read().ok_or(SyscallError::EFAULT)?;
            let mut mgr = SHM_MANAGER.lock();
            mgr.shmctl(shmid, crate::ipc::shm::IPC_SET, None, Some(&ds), None, uid, gid)?;
            Ok(0)
        }
        crate::ipc::shm::IPC_INFO | crate::ipc::shm::SHM_INFO => {
            let info_uptr = UserPtr::<ShmInfo>::from_u64(buf_ptr);
            if !info_uptr.is_valid() {
                return Err(SyscallError::EFAULT);
            }
            let mut info = ShmInfo::default();
            let res = {
                let mut mgr = SHM_MANAGER.lock();
                mgr.shmctl(shmid, cmd_stripped, None, None, Some(&mut info), uid, gid)?
            };
            info_uptr.write(info).ok_or(SyscallError::EFAULT)?;
            Ok(res as usize)
        }
        crate::ipc::shm::SHM_LOCK | crate::ipc::shm::SHM_UNLOCK => {
            let mut mgr = SHM_MANAGER.lock();
            mgr.shmctl(shmid, cmd_stripped, None, None, None, uid, gid)?;
            Ok(0)
        }
        _ => Err(SyscallError::EINVAL),
    }
}

