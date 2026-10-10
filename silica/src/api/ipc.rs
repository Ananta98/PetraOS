// SPDX-License-Identifier: GPL-2.0

//! System V IPC common ABI definitions.
//!
//! Provides common constants, keys, permissions (`struct ipc_perm` / `struct ipc64_perm`),
//! and operation flags shared across System V Shared Memory, Semaphores, and Message Queues.

use crate::api::cred::Ucred;

/// System V IPC key type (`key_t` in C).
pub type Key = i32;

/// Private IPC key: always creates a new, unshared IPC object.
pub const IPC_PRIVATE: Key = 0;

/// Create key if key does not exist.
pub const IPC_CREAT: i32 = 0o1000; // 0x200 (512)

/// Fail if key exists and `IPC_CREAT` is set.
pub const IPC_EXCL: i32 = 0o2000; // 0x400 (1024)

/// Return error if operation must wait/block.
pub const IPC_NOWAIT: i32 = 0o4000; // 0x800 (2048)

/// Remove identifier.
pub const IPC_RMID: i32 = 0;

/// Set options / permissions.
pub const IPC_SET: i32 = 1;

/// Get `ipc_perm` options and information.
pub const IPC_STAT: i32 = 2;

/// See `ipcs` info.
pub const IPC_INFO: i32 = 3;

/// Permission mode mask for user, group, other (0o777).
pub const IPC_MODE_MASK: u32 = 0o777;

/// Linux x86_64 compatible `struct ipc64_perm` (48 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IpcPerm {
    /// Key supplied to `shmget` or `semget`.
    pub key: Key,
    /// Effective UID of owner.
    pub uid: u32,
    /// Effective GID of owner.
    pub gid: u32,
    /// Effective UID of creator.
    pub cuid: u32,
    /// Effective GID of creator.
    pub cgid: u32,
    /// Permissions mode (low 9 bits) + flags.
    pub mode: u32,
    /// Sequence number.
    pub seq: u16,
    /// Alignment padding.
    pub pad: [u8; 6],
    /// Reserved for glibc.
    pub glibc_reserved1: u64,
    /// Reserved for glibc.
    pub glibc_reserved2: u64,
}

impl IpcPerm {
    /// Creates a new `IpcPerm` initialized for the given creator credentials, key, and mode.
    pub fn new(key: Key, cred: &Ucred, mode: u32) -> Self {
        Self {
            key,
            uid: cred.cr_uid.as_u32(),
            gid: cred.cr_gid.as_u32(),
            cuid: cred.cr_uid.as_u32(),
            cgid: cred.cr_gid.as_u32(),
            mode: mode & IPC_MODE_MASK,
            seq: 0,
            pad: [0u8; 6],
            glibc_reserved1: 0,
            glibc_reserved2: 0,
        }
    }

    /// Checks whether `cred` has permissions to perform an operation with requested mode bits
    /// (e.g. read=0o400/0o040/0o004, write=0o200/0o020/0o002).
    pub fn check_perm(&self, cred: &Ucred, req_mode: u32) -> bool {
        if cred.cr_uid.is_root() {
            return true;
        }

        let mut granted = 0u32;
        if cred.cr_uid.as_u32() == self.uid || cred.cr_uid.as_u32() == self.cuid {
            granted |= (self.mode >> 6) & 0o7;
        } else if cred.cr_gid.as_u32() == self.gid || cred.cr_gid.as_u32() == self.cgid {
            granted |= (self.mode >> 3) & 0o7;
        } else {
            granted |= self.mode & 0o7;
        }

        (granted & req_mode) == req_mode
    }

    /// Serializes `IpcPerm` into a 48-byte array without unsafe code.
    pub fn to_bytes(&self) -> [u8; 48] {
        let mut buf = [0u8; 48];
        buf[0..4].copy_from_slice(&self.key.to_ne_bytes());
        buf[4..8].copy_from_slice(&self.uid.to_ne_bytes());
        buf[8..12].copy_from_slice(&self.gid.to_ne_bytes());
        buf[12..16].copy_from_slice(&self.cuid.to_ne_bytes());
        buf[16..20].copy_from_slice(&self.cgid.to_ne_bytes());
        buf[20..24].copy_from_slice(&self.mode.to_ne_bytes());
        buf[24..26].copy_from_slice(&self.seq.to_ne_bytes());
        buf[26..32].copy_from_slice(&self.pad);
        buf[32..40].copy_from_slice(&self.glibc_reserved1.to_ne_bytes());
        buf[40..48].copy_from_slice(&self.glibc_reserved2.to_ne_bytes());
        buf
    }

    /// Deserializes `IpcPerm` from a byte slice safely.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 48 {
            return None;
        }
        let key = i32::from_ne_bytes(bytes[0..4].try_into().ok()?);
        let uid = u32::from_ne_bytes(bytes[4..8].try_into().ok()?);
        let gid = u32::from_ne_bytes(bytes[8..12].try_into().ok()?);
        let cuid = u32::from_ne_bytes(bytes[12..16].try_into().ok()?);
        let cgid = u32::from_ne_bytes(bytes[16..20].try_into().ok()?);
        let mode = u32::from_ne_bytes(bytes[20..24].try_into().ok()?);
        let seq = u16::from_ne_bytes(bytes[24..26].try_into().ok()?);
        let mut pad = [0u8; 6];
        pad.copy_from_slice(&bytes[26..32]);
        let glibc_reserved1 = u64::from_ne_bytes(bytes[32..40].try_into().ok()?);
        let glibc_reserved2 = u64::from_ne_bytes(bytes[40..48].try_into().ok()?);

        Some(Self {
            key,
            uid,
            gid,
            cuid,
            cgid,
            mode,
            seq,
            pad,
            glibc_reserved1,
            glibc_reserved2,
        })
    }
}
