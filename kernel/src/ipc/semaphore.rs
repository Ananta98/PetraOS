//! System V IPC Semaphore Subsystem
//!
//! Implements POSIX/SysV semaphore sets with `semget`, `semop`, `semctl`, and
//! `semtimedop` semantics. Semaphores are kernel-managed counting primitives
//! used for inter-process synchronization.
//!
//! Design:
//! - A *semaphore set* (identified by a numeric `semid`) contains N individual
//!   semaphores, each with an unsigned 16-bit value and associated wait queues.
//! - `semget`: creates or opens a set by `key_t` (or `IPC_PRIVATE`).
//! - `semop` / `semtimedop`: atomically applies a batch of `sembuf` operations;
//!   blocks the calling thread if any operation cannot proceed.
//! - `semctl`: performs control or query operations on a semaphore set.
//!
//! Thread blocking uses the kernel futex/scheduler infrastructure so sleeping
//! threads are properly descheduled and do not spin-waste CPU.

use crate::arch::timer::hpet;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicI32, Ordering};

use crate::proc::thread::{Thread, ThreadState};
use crate::sync::spinlock::Spinlock;

// ── IPC flags / commands ─────────────────────────────────────────────────────

pub const IPC_PRIVATE: i32 = 0;
pub const IPC_CREAT: i32 = 0o1000;
pub const IPC_EXCL: i32 = 0o2000;
pub const IPC_NOWAIT: i16 = 0o4000_u16 as i16;
pub const IPC_RMID: i32 = 0;
pub const IPC_SET: i32 = 1;
pub const IPC_STAT: i32 = 2;

pub const SEM_UNDO: i16 = 0x1000_u16 as i16;

pub const GETPID: i32 = 11;
pub const GETVAL: i32 = 12;
pub const GETALL: i32 = 13;
pub const GETNCNT: i32 = 14;
pub const GETZCNT: i32 = 15;
pub const SETVAL: i32 = 16;
pub const SETALL: i32 = 17;

/// Maximum value a semaphore can hold.
pub const SEM_VALUE_MAX: u16 = 32767;
/// Maximum number of semaphores per set.
pub const SEMMSL: usize = 250;

// ── Error type ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemError {
    /// Invalid argument
    InvalidArg,
    /// No entity (semaphore set removed or does not exist)
    NotFound,
    /// Permission denied
    PermDenied,
    /// Already exists (IPC_EXCL)
    AlreadyExists,
    /// Operation would overflow semaphore value
    Overflow,
    /// Resource limit reached (no semaphore IDs left)
    OutOfIds,
    /// Would block (IPC_NOWAIT requested)
    WouldBlock,
    /// Semaphore set was removed while waiting
    Removed,
    /// Out of memory
    NoMem,
}

// ── ABI-compatible IPC permission structure ───────────────────────────────────

/// Mirrors `struct ipc_perm` from userspace ABI.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct IpcPerm {
    pub key: i32,
    pub uid: u32,
    pub gid: u32,
    pub cuid: u32,
    pub cgid: u32,
    pub mode: u32,
    pub seq: u32,
    _pad: [u64; 2],
}

/// Mirrors `struct semid_ds` from userspace ABI.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct SemidDs {
    pub sem_perm: IpcPerm,
    pub sem_otime: i64,
    pub sem_ctime: i64,
    pub sem_nsems: u64,
    _pad: [u64; 2],
}

/// Single `sembuf` operation descriptor.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct SemBuf {
    /// Semaphore index within the set
    pub sem_num: u16,
    /// Operation: >0 add, <0 subtract, 0 wait-for-zero
    pub sem_op: i16,
    /// Flags: IPC_NOWAIT, SEM_UNDO
    pub sem_flg: i16,
}

// ── Per-semaphore state ───────────────────────────────────────────────────────

/// State for a single semaphore within a set.
struct SemState {
    /// Current value (0..=SEM_VALUE_MAX)
    value: u16,
    /// PID of the last process to operate on this semaphore
    sempid: u32,
    /// Threads waiting for value > 0 (sem_op < 0 or > 0)
    ncnt_waiters: VecDeque<Arc<Spinlock<Thread>>>,
    /// Threads waiting for value == 0
    zcnt_waiters: VecDeque<Arc<Spinlock<Thread>>>,
}

impl SemState {
    fn new(initial: u16) -> Self {
        Self {
            value: initial,
            sempid: 0,
            ncnt_waiters: VecDeque::new(),
            zcnt_waiters: VecDeque::new(),
        }
    }
}

// ── Semaphore set ─────────────────────────────────────────────────────────────

/// A System V semaphore set holding one or more semaphores.
pub struct SemSet {
    /// Unique semaphore-set ID
    pub id: i32,
    /// IPC key (IPC_PRIVATE → 0)
    pub key: i32,
    /// IPC permissions and metadata
    pub perm: IpcPerm,
    /// Array of semaphores in this set
    sems: Vec<SemState>,
    /// Time of last successful `semop` (seconds since epoch)
    pub otime: i64,
    /// Time of last `semctl` change
    pub ctime: i64,
    /// Set to true when the set has been IPC_RMID'd; wakes blocked waiters
    pub removed: bool,
}

impl SemSet {
    fn new(id: i32, key: i32, nsems: usize, uid: u32, gid: u32, mode: u32) -> Self {
        let mut sems = Vec::with_capacity(nsems);
        for _ in 0..nsems {
            sems.push(SemState::new(0));
        }
        Self {
            id,
            key,
            perm: IpcPerm {
                key,
                uid,
                gid,
                cuid: uid,
                cgid: gid,
                mode: mode & 0o777,
                seq: 0,
                _pad: [0; 2],
            },
            sems,
            otime: 0,
            ctime: hpet::elapsed_ns() as i64 / 1_000_000_000,
            removed: false,
        }
    }

    pub fn nsems(&self) -> usize {
        self.sems.len()
    }

    /// Build a `SemidDs` snapshot of this set.
    pub fn semid_ds(&self) -> SemidDs {
        SemidDs {
            sem_perm: self.perm,
            sem_otime: self.otime,
            sem_ctime: self.ctime,
            sem_nsems: self.nsems() as u64,
            _pad: [0; 2],
        }
    }

    /// Wake threads whose conditions are now satisfied after a value change.
    fn wake_waiters(&mut self, sem_idx: usize) {
        let val = self.sems[sem_idx].value;

        // Wake zero-waiters if value dropped to zero
        if val == 0 {
            while let Some(thread) = self.sems[sem_idx].zcnt_waiters.pop_front() {
                Thread::unblock(thread);
            }
        }

        // Wake ncnt-waiters (waiting for value > 0) if value is now positive
        if val > 0 {
            // Wake them all; they will re-check the value atomically
            while let Some(thread) = self.sems[sem_idx].ncnt_waiters.pop_front() {
                Thread::unblock(thread);
            }
        }
    }

    /// Check whether a slice of sembufs can be applied without blocking.
    /// Returns `Ok(())` if all ops are satisfiable right now, or `Err(idx)`
    /// where `idx` is the first blocking op.
    fn can_apply_nowait(&self, ops: &[SemBuf]) -> Result<(), usize> {
        for (i, op) in ops.iter().enumerate() {
            let idx = op.sem_num as usize;
            let val = self.sems[idx].value as i32;
            let so = op.sem_op as i32;
            if so < 0 {
                // Needs val >= |op|
                if val + so < 0 {
                    return Err(i);
                }
            } else if so == 0 {
                // Needs val == 0
                if val != 0 {
                    return Err(i);
                }
            }
            // so > 0 always succeeds (may saturate – checked at apply time)
        }
        Ok(())
    }

    /// Apply a slice of sembufs atomically (called only when can_apply_nowait succeeded).
    /// Returns the updated `sempid` values.
    fn apply(&mut self, ops: &[SemBuf], pid: u32) {
        for op in ops {
            let idx = op.sem_num as usize;
            let so = op.sem_op as i32;
            let cur = self.sems[idx].value as i32;
            let new_val = (cur + so).clamp(0, SEM_VALUE_MAX as i32) as u16;
            self.sems[idx].value = new_val;
            self.sems[idx].sempid = pid;
            self.wake_waiters(idx);
        }
        self.otime = hpet::elapsed_ns() as i64 / 1_000_000_000;
    }
}

// ── Global semaphore manager ──────────────────────────────────────────────────

/// Global allocation counter for semaphore set IDs.
static NEXT_SEMID: AtomicI32 = AtomicI32::new(1);

/// Singleton manager holding all live semaphore sets.
pub struct SemaphoreManager {
    /// Map from semid → SemSet
    pub(crate) sets: BTreeMap<i32, SemSet>,
    /// Map from key → semid (for named lookup; excludes IPC_PRIVATE)
    pub(crate) key_map: BTreeMap<i32, i32>,
}

impl SemaphoreManager {
    pub const fn new() -> Self {
        Self {
            sets: BTreeMap::new(),
            key_map: BTreeMap::new(),
        }
    }

    // ── semget ────────────────────────────────────────────────────────────────

    /// Implements `semget(key, nsems, semflg)`.
    pub fn semget(
        &mut self,
        key: i32,
        nsems: i32,
        semflg: i32,
        uid: u32,
        gid: u32,
    ) -> Result<i32, SemError> {
        if nsems < 0 || nsems as usize > SEMMSL {
            return Err(SemError::InvalidArg);
        }

        if key != IPC_PRIVATE {
            // Look up existing set by key
            if let Some(&existing_id) = self.key_map.get(&key) {
                if (semflg & IPC_CREAT) != 0 && (semflg & IPC_EXCL) != 0 {
                    return Err(SemError::AlreadyExists);
                }
                let set = self.sets.get(&existing_id).ok_or(SemError::NotFound)?;
                if nsems > 0 && nsems as usize > set.nsems() {
                    return Err(SemError::InvalidArg);
                }
                return Ok(existing_id);
            }
            // If we reach here and IPC_CREAT is not set, it doesn't exist
            if (semflg & IPC_CREAT) == 0 {
                return Err(SemError::NotFound);
            }
        }

        // Create new set
        if nsems == 0 {
            return Err(SemError::InvalidArg);
        }

        let id = NEXT_SEMID.fetch_add(1, Ordering::Relaxed);
        let mode = (semflg & 0o777) as u32;
        let set = SemSet::new(id, key, nsems as usize, uid, gid, mode);
        self.sets.insert(id, set);
        if key != IPC_PRIVATE {
            self.key_map.insert(key, id);
        }
        Ok(id)
    }

    // ── semctl ────────────────────────────────────────────────────────────────

    /// Implements `semctl(semid, semnum, cmd, arg)`.
    /// `arg_val` is used for SETVAL; `arg_ds` for IPC_SET/STAT; `arg_array` for GETALL/SETALL.
    pub fn semctl(
        &mut self,
        semid: i32,
        semnum: i32,
        cmd: i32,
        arg_val: Option<i32>,
        arg_ds: Option<&mut SemidDs>,
        arg_array: Option<&[u16]>,
        out_array: Option<&mut [u16]>,
        uid: u32,
    ) -> Result<i32, SemError> {
        let set = self.sets.get_mut(&semid).ok_or(SemError::NotFound)?;

        match cmd {
            IPC_RMID => {
                // Mark removed and wake all blocked threads
                set.removed = true;
                for s in set.sems.iter_mut() {
                    while let Some(t) = s.ncnt_waiters.pop_front() {
                        Thread::unblock(t);
                    }
                    while let Some(t) = s.zcnt_waiters.pop_front() {
                        Thread::unblock(t);
                    }
                }
                let key = set.key;
                self.sets.remove(&semid);
                if key != IPC_PRIVATE {
                    self.key_map.remove(&key);
                }
                Ok(0)
            }

            IPC_STAT => {
                if let Some(ds) = arg_ds {
                    *ds = set.semid_ds();
                }
                Ok(0)
            }

            IPC_SET => {
                if let Some(ds) = arg_ds {
                    set.perm.uid = ds.sem_perm.uid;
                    set.perm.gid = ds.sem_perm.gid;
                    set.perm.mode = ds.sem_perm.mode & 0o777;
                    set.ctime = crate::arch::timer::hpet::elapsed_ns() as i64 / 1_000_000_000;
                }
                Ok(0)
            }

            GETVAL => {
                let idx = semnum as usize;
                if idx >= set.nsems() {
                    return Err(SemError::InvalidArg);
                }
                Ok(set.sems[idx].value as i32)
            }

            SETVAL => {
                let idx = semnum as usize;
                if idx >= set.nsems() {
                    return Err(SemError::InvalidArg);
                }
                let v = arg_val.ok_or(SemError::InvalidArg)?;
                if v < 0 || v as u32 > SEM_VALUE_MAX as u32 {
                    return Err(SemError::Overflow);
                }
                set.sems[idx].value = v as u16;
                set.sems[idx].sempid = uid; // Linux sets sempid on SETVAL
                set.wake_waiters(idx);
                set.ctime = crate::arch::timer::hpet::elapsed_ns() as i64 / 1_000_000_000;
                Ok(0)
            }

            GETALL => {
                if let Some(out) = out_array {
                    if out.len() < set.nsems() {
                        return Err(SemError::InvalidArg);
                    }
                    for (i, s) in set.sems.iter().enumerate() {
                        out[i] = s.value;
                    }
                }
                Ok(0)
            }

            SETALL => {
                let arr = arg_array.ok_or(SemError::InvalidArg)?;
                if arr.len() < set.nsems() {
                    return Err(SemError::InvalidArg);
                }
                for (i, &v) in arr.iter().enumerate().take(set.nsems()) {
                    if v > SEM_VALUE_MAX {
                        return Err(SemError::Overflow);
                    }
                    set.sems[i].value = v;
                    set.wake_waiters(i);
                }
                set.ctime = crate::arch::timer::hpet::elapsed_ns() as i64 / 1_000_000_000;
                Ok(0)
            }

            GETPID => {
                let idx = semnum as usize;
                if idx >= set.nsems() {
                    return Err(SemError::InvalidArg);
                }
                Ok(set.sems[idx].sempid as i32)
            }

            GETNCNT => {
                let idx = semnum as usize;
                if idx >= set.nsems() {
                    return Err(SemError::InvalidArg);
                }
                Ok(set.sems[idx].ncnt_waiters.len() as i32)
            }

            GETZCNT => {
                let idx = semnum as usize;
                if idx >= set.nsems() {
                    return Err(SemError::InvalidArg);
                }
                Ok(set.sems[idx].zcnt_waiters.len() as i32)
            }

            _ => Err(SemError::InvalidArg),
        }
    }

    // ── semop / semtimedop ────────────────────────────────────────────────────

    /// Try to apply the `ops` to semaphore set `semid`.
    ///
    /// Returns `Ok(SemopResult::Done)` if all operations succeeded immediately,
    /// or `Ok(SemopResult::Block { thread, semid, ops })` if the calling thread
    /// must be put to sleep and retried after being woken.
    /// Returns `Err` on hard failures.
    pub fn semop_try(
        &mut self,
        semid: i32,
        ops: &[SemBuf],
        thread: Arc<Spinlock<Thread>>,
        pid: u32,
        nowait: bool,
    ) -> Result<SemopResult, SemError> {
        if ops.is_empty() {
            return Ok(SemopResult::Done);
        }

        let set = self.sets.get_mut(&semid).ok_or(SemError::NotFound)?;

        if set.removed {
            return Err(SemError::Removed);
        }

        // Validate all indices upfront
        for op in ops {
            if op.sem_num as usize >= set.nsems() {
                return Err(SemError::InvalidArg);
            }
        }

        match set.can_apply_nowait(ops) {
            Ok(()) => {
                set.apply(ops, pid);
                Ok(SemopResult::Done)
            }
            Err(blocking_idx) => {
                // If IPC_NOWAIT is requested on the blocking op, return immediately
                if nowait || (ops[blocking_idx].sem_flg & IPC_NOWAIT) != 0 {
                    return Err(SemError::WouldBlock);
                }

                // Enqueue thread in the appropriate waiter queue
                let blocking_op = &ops[blocking_idx];
                let sem_idx = blocking_op.sem_num as usize;
                if blocking_op.sem_op == 0 {
                    set.sems[sem_idx].zcnt_waiters.push_back(thread);
                } else {
                    set.sems[sem_idx].ncnt_waiters.push_back(thread);
                }

                Ok(SemopResult::Block { semid })
            }
        }
    }

    /// Called after a thread is woken up: re-try applying ops.
    /// Returns `true` if the semaphore set was removed while waiting.
    pub fn semop_retry(&mut self, semid: i32, ops: &[SemBuf], pid: u32) -> Result<bool, SemError> {
        let set = self.sets.get_mut(&semid).ok_or(SemError::NotFound)?;

        if set.removed {
            return Ok(true); // signal caller to return EIDRM
        }

        match set.can_apply_nowait(ops) {
            Ok(()) => {
                set.apply(ops, pid);
                Ok(false)
            }
            Err(_) => {
                // Still blocked – caller must re-sleep
                Err(SemError::WouldBlock)
            }
        }
    }
}

/// Result type for `semop_try`.
pub enum SemopResult {
    /// All operations applied immediately.
    Done,
    /// Thread must block on the semaphore set identified by `semid`.
    Block { semid: i32 },
}

/// Global semaphore manager singleton.
pub static SEMAPHORE_MANAGER: Spinlock<SemaphoreManager> = Spinlock::new(SemaphoreManager::new());
