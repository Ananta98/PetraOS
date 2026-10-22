// SPDX-License-Identifier: GPL-2.0

//! System V Semaphore ABI definitions.
//!
//! Defines `struct sembuf`, `struct semid_ds`, `struct seminfo`, and commands (`GETVAL`,
//! `SETVAL`, `SEM_UNDO`, etc.) according to the Linux x86-64 ABI.

use super::ipc::IpcPerm;

/// Set up undo structure on process exit.
pub const SEM_UNDO: i16 = 0x1000; // 4096

/// Return the value of `sempid`.
pub const GETPID: i32 = 11;

/// Return the value of `semval`.
pub const GETVAL: i32 = 12;

/// Return all `semval`s for the set.
pub const GETALL: i32 = 13;

/// Return the value of `semncnt`.
pub const GETNCNT: i32 = 14;

/// Return the value of `semzcnt`.
pub const GETZCNT: i32 = 15;

/// Set the value of `semval` to `arg.val`.
pub const SETVAL: i32 = 16;

/// Set all `semval`s for the set to `arg.array`.
pub const SETALL: i32 = 17;

/// Get `semid_ds` by index in global array.
pub const SEM_STAT: i32 = 18;

/// See `ipcs` info.
pub const SEM_INFO: i32 = 19;

/// Get `semid_ds` by index without read permission check.
pub const SEM_STAT_ANY: i32 = 20;

/// Maximum semaphore value.
pub const SEMVMX: u16 = 32767;

/// Maximum adjust-on-exit value.
pub const SEMAEM: i16 = 16384;

/// Maximum number of semaphores per semaphore set.
pub const SEMMSL: usize = 32000;

/// Maximum number of semaphore sets system-wide.
pub const SEMMNI: usize = 32000;

/// Maximum number of semaphores system-wide (`SEMMSL * SEMMNI`).
pub const SEMMNS: usize = 1024 * 1024;

/// Maximum number of operations per `semop` call.
pub const SEMOPM: usize = 32;

/// Maximum number of undo structures per process.
pub const SEMVMU: usize = 32;

/// Linux compatible `struct sembuf` (6 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SemBuf {
    /// Semaphore number in set (0-indexed).
    pub sem_num: u16,
    /// Semaphore operation: >0 increment, 0 wait-for-zero, <0 decrement.
    pub sem_op: i16,
    /// Operation flags (`IPC_NOWAIT`, `SEM_UNDO`).
    pub sem_flg: i16,
}

impl SemBuf {
    /// Serializes `SemBuf` into a 6-byte array without unsafe code.
    pub fn to_bytes(&self) -> [u8; 6] {
        let mut buf = [0u8; 6];
        buf[0..2].copy_from_slice(&self.sem_num.to_ne_bytes());
        buf[2..4].copy_from_slice(&self.sem_op.to_ne_bytes());
        buf[4..6].copy_from_slice(&self.sem_flg.to_ne_bytes());
        buf
    }

    /// Deserializes `SemBuf` from a byte slice safely.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 6 {
            return None;
        }
        let sem_num = u16::from_ne_bytes(bytes[0..2].try_into().ok()?);
        let sem_op = i16::from_ne_bytes(bytes[2..4].try_into().ok()?);
        let sem_flg = i16::from_ne_bytes(bytes[4..6].try_into().ok()?);
        Some(Self {
            sem_num,
            sem_op,
            sem_flg,
        })
    }
}

/// Linux x86_64 compatible `struct semid_ds` (104 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SemIdDs {
    /// Ownership and permissions.
    pub sem_perm: IpcPerm,
    /// Last `semop` time.
    pub sem_otime: i64,
    /// Reserved for glibc.
    pub glibc_reserved1: u64,
    /// Last change time.
    pub sem_ctime: i64,
    /// Reserved for glibc.
    pub glibc_reserved2: u64,
    /// Number of semaphores in set.
    pub sem_nsems: usize,
    /// Reserved for glibc.
    pub glibc_reserved3: u64,
    /// Reserved for glibc.
    pub glibc_reserved4: u64,
}

impl SemIdDs {
    /// Serializes `SemIdDs` into a 104-byte array without unsafe code.
    pub fn to_bytes(&self) -> [u8; 104] {
        let mut buf = [0u8; 104];
        buf[0..48].copy_from_slice(&self.sem_perm.to_bytes());
        buf[48..56].copy_from_slice(&self.sem_otime.to_ne_bytes());
        buf[56..64].copy_from_slice(&self.glibc_reserved1.to_ne_bytes());
        buf[64..72].copy_from_slice(&self.sem_ctime.to_ne_bytes());
        buf[72..80].copy_from_slice(&self.glibc_reserved2.to_ne_bytes());
        buf[80..88].copy_from_slice(&self.sem_nsems.to_ne_bytes());
        buf[88..96].copy_from_slice(&self.glibc_reserved3.to_ne_bytes());
        buf[96..104].copy_from_slice(&self.glibc_reserved4.to_ne_bytes());
        buf
    }

    /// Deserializes `SemIdDs` from a byte slice safely.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 104 {
            return None;
        }
        let sem_perm = IpcPerm::from_bytes(&bytes[0..48])?;
        let sem_otime = i64::from_ne_bytes(bytes[48..56].try_into().ok()?);
        let glibc_reserved1 = u64::from_ne_bytes(bytes[56..64].try_into().ok()?);
        let sem_ctime = i64::from_ne_bytes(bytes[64..72].try_into().ok()?);
        let glibc_reserved2 = u64::from_ne_bytes(bytes[72..80].try_into().ok()?);
        let sem_nsems = usize::from_ne_bytes(bytes[80..88].try_into().ok()?);
        let glibc_reserved3 = u64::from_ne_bytes(bytes[88..96].try_into().ok()?);
        let glibc_reserved4 = u64::from_ne_bytes(bytes[96..104].try_into().ok()?);

        Some(Self {
            sem_perm,
            sem_otime,
            glibc_reserved1,
            sem_ctime,
            glibc_reserved2,
            sem_nsems,
            glibc_reserved3,
            glibc_reserved4,
        })
    }
}

/// Linux `struct seminfo` describing system-wide limits (40 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SemInfo {
    pub semmap: i32,
    pub semmni: i32,
    pub semmns: i32,
    pub semmnu: i32,
    pub semmsl: i32,
    pub semopm: i32,
    pub semume: i32,
    pub semusz: i32,
    pub semvmx: i32,
    pub semaem: i32,
}

impl Default for SemInfo {
    fn default() -> Self {
        Self {
            semmap: 0,
            semmni: SEMMNI as i32,
            semmns: SEMMNS as i32,
            semmnu: 0,
            semmsl: SEMMSL as i32,
            semopm: SEMOPM as i32,
            semume: 0,
            semusz: 0,
            semvmx: SEMVMX as i32,
            semaem: SEMAEM as i32,
        }
    }
}

impl SemInfo {
    /// Serializes `SemInfo` into a 40-byte array without unsafe code.
    pub fn to_bytes(&self) -> [u8; 40] {
        let mut buf = [0u8; 40];
        buf[0..4].copy_from_slice(&self.semmap.to_ne_bytes());
        buf[4..8].copy_from_slice(&self.semmni.to_ne_bytes());
        buf[8..12].copy_from_slice(&self.semmns.to_ne_bytes());
        buf[12..16].copy_from_slice(&self.semmnu.to_ne_bytes());
        buf[16..20].copy_from_slice(&self.semmsl.to_ne_bytes());
        buf[20..24].copy_from_slice(&self.semopm.to_ne_bytes());
        buf[24..28].copy_from_slice(&self.semume.to_ne_bytes());
        buf[28..32].copy_from_slice(&self.semusz.to_ne_bytes());
        buf[32..36].copy_from_slice(&self.semvmx.to_ne_bytes());
        buf[36..40].copy_from_slice(&self.semaem.to_ne_bytes());
        buf
    }
}
