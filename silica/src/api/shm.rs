// SPDX-License-Identifier: GPL-2.0

//! System V Shared Memory ABI definitions.
//!
//! Defines `struct shmid_ds`, `struct shminfo`, and operation flags (`SHM_RDONLY`, `SHM_RND`,
//! `SHM_REMAP`, `SHM_DEST`, etc.) according to the Linux x86-64 ABI.

use super::ipc::IpcPerm;

/// Attach shared memory read-only.
pub const SHM_RDONLY: i32 = 0o10000; // 0x1000 (4096)

/// Round attach address to SHMLBA.
pub const SHM_RND: i32 = 0o20000; // 0x2000 (8192)

/// Take over existing mapping on attach.
pub const SHM_REMAP: i32 = 0o40000; // 0x4000 (16384)

/// Execution permission.
pub const SHM_EXEC: i32 = 0o100000; // 0x8000 (32768)

/// Lock segment in memory.
pub const SHM_LOCK: i32 = 11;

/// Unlock segment.
pub const SHM_UNLOCK: i32 = 12;

/// Get `shmid_ds` by index in global table.
pub const SHM_STAT: i32 = 13;

/// Get system-wide `shm_info`.
pub const SHM_INFO: i32 = 14;

/// Get `shmid_ds` by index without read permission check.
pub const SHM_STAT_ANY: i32 = 15;

/// Segment is destroyed / marked for deletion.
pub const SHM_DEST: u32 = 0o1000;

/// Segment is locked into memory.
pub const SHM_LOCKED: u32 = 0o2000;

/// Segment low boundary address multiple (typically 1 page = 4096 bytes).
pub const SHMLBA: usize = 4096;

/// Default maximum size in bytes of a single shared memory segment (1 GiB).
pub const SHMMAX: usize = 1024 * 1024 * 1024;

/// Default minimum size in bytes of a shared memory segment.
pub const SHMMIN: usize = 1;

/// Maximum number of shared memory segments system-wide.
pub const SHMMNI: usize = 4096;

/// Maximum number of shared memory segments attached per process.
pub const SHMSEG: usize = 4096;

/// Total system-wide shared memory in pages (8 GiB / 4096).
pub const SHMALL: usize = 2 * 1024 * 1024;

/// Linux x86_64 compatible `struct shmid_ds` (112 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShmIdDs {
    /// Ownership and permissions.
    pub shm_perm: IpcPerm,
    /// Size of segment in bytes.
    pub shm_segsz: usize,
    /// Last attach time.
    pub shm_atime: i64,
    /// Last detach time.
    pub shm_dtime: i64,
    /// Last change time.
    pub shm_ctime: i64,
    /// PID of creator.
    pub shm_cpid: u32,
    /// PID of last operator (`shmat` / `shmdt`).
    pub shm_lpid: u32,
    /// Number of current attachments.
    pub shm_nattch: usize,
    /// Reserved for glibc.
    pub glibc_reserved4: u64,
    /// Reserved for glibc.
    pub glibc_reserved5: u64,
}

impl ShmIdDs {
    /// Serializes `ShmIdDs` into a 112-byte array without unsafe code.
    pub fn to_bytes(&self) -> [u8; 112] {
        let mut buf = [0u8; 112];
        buf[0..48].copy_from_slice(&self.shm_perm.to_bytes());
        buf[48..56].copy_from_slice(&self.shm_segsz.to_ne_bytes());
        buf[56..64].copy_from_slice(&self.shm_atime.to_ne_bytes());
        buf[64..72].copy_from_slice(&self.shm_dtime.to_ne_bytes());
        buf[72..80].copy_from_slice(&self.shm_ctime.to_ne_bytes());
        buf[80..84].copy_from_slice(&self.shm_cpid.to_ne_bytes());
        buf[84..88].copy_from_slice(&self.shm_lpid.to_ne_bytes());
        buf[88..96].copy_from_slice(&self.shm_nattch.to_ne_bytes());
        buf[96..104].copy_from_slice(&self.glibc_reserved4.to_ne_bytes());
        buf[104..112].copy_from_slice(&self.glibc_reserved5.to_ne_bytes());
        buf
    }

    /// Deserializes `ShmIdDs` from a byte slice safely.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 112 {
            return None;
        }
        let shm_perm = IpcPerm::from_bytes(&bytes[0..48])?;
        let shm_segsz = usize::from_ne_bytes(bytes[48..56].try_into().ok()?);
        let shm_atime = i64::from_ne_bytes(bytes[56..64].try_into().ok()?);
        let shm_dtime = i64::from_ne_bytes(bytes[64..72].try_into().ok()?);
        let shm_ctime = i64::from_ne_bytes(bytes[72..80].try_into().ok()?);
        let shm_cpid = u32::from_ne_bytes(bytes[80..84].try_into().ok()?);
        let shm_lpid = u32::from_ne_bytes(bytes[84..88].try_into().ok()?);
        let shm_nattch = usize::from_ne_bytes(bytes[88..96].try_into().ok()?);
        let glibc_reserved4 = u64::from_ne_bytes(bytes[96..104].try_into().ok()?);
        let glibc_reserved5 = u64::from_ne_bytes(bytes[104..112].try_into().ok()?);

        Some(Self {
            shm_perm,
            shm_segsz,
            shm_atime,
            shm_dtime,
            shm_ctime,
            shm_cpid,
            shm_lpid,
            shm_nattch,
            glibc_reserved4,
            glibc_reserved5,
        })
    }
}

/// Linux `struct shminfo` describing system-wide limits (40 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShmInfo {
    pub shmmax: u64,
    pub shmmin: u64,
    pub shmmni: u64,
    pub shmseg: u64,
    pub shmall: u64,
}

impl Default for ShmInfo {
    fn default() -> Self {
        Self {
            shmmax: SHMMAX as u64,
            shmmin: SHMMIN as u64,
            shmmni: SHMMNI as u64,
            shmseg: SHMSEG as u64,
            shmall: SHMALL as u64,
        }
    }
}

impl ShmInfo {
    /// Serializes `ShmInfo` to a 40-byte array without unsafe code.
    pub fn to_bytes(&self) -> [u8; 40] {
        let mut buf = [0u8; 40];
        buf[0..8].copy_from_slice(&self.shmmax.to_ne_bytes());
        buf[8..16].copy_from_slice(&self.shmmin.to_ne_bytes());
        buf[16..24].copy_from_slice(&self.shmmni.to_ne_bytes());
        buf[24..32].copy_from_slice(&self.shmseg.to_ne_bytes());
        buf[32..40].copy_from_slice(&self.shmall.to_ne_bytes());
        buf
    }
}
