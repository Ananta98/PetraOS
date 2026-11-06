// SPDX-License-Identifier: GPL-2.0

//! System V Message Queue ABI definitions.
//!
//! Matches Linux x86-64 `struct msqid_ds` (120 bytes) and `struct msginfo`.

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use super::ipc::IpcPerm;

/// Maximum bytes allowed in a single message by default.
pub const MSGMAX: usize = 8192;
/// Default maximum total bytes in a message queue.
pub const MSGMNB: usize = 16384;
/// Maximum number of message queues system-wide.
pub const MSGMNI: usize = 32000;

/// `msgrcv(2)` flag: truncate message if it is bigger than `msgsz`.
pub const MSG_NOERROR: i32 = 0o10000;
/// `msgrcv(2)` flag: read any message except the one with `msgtyp`.
pub const MSG_EXCEPT: i32 = 0o20000;
/// `msgrcv(2)` flag: copy message without removing it from queue.
pub const MSG_COPY: i32 = 0o40000;

/// Linux command: return `msqid_ds` for IPC object by index.
pub const MSG_STAT: i32 = 11;
/// Linux command: return `msginfo` structure with system limits.
pub const MSG_INFO: i32 = 12;

/// Linux x86-64 compatible `struct msqid_ds` (120 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct MsqIdDs {
    /// Ownership and permissions (48 bytes).
    pub msg_perm: IpcPerm,
    /// Time of last `msgsnd` (8 bytes).
    pub msg_stime: i64,
    /// Time of last `msgrcv` (8 bytes).
    pub msg_rtime: i64,
    /// Time of last change (8 bytes).
    pub msg_ctime: i64,
    /// Current number of bytes in queue (8 bytes).
    pub msg_cbytes: u64,
    /// Current number of messages in queue (8 bytes).
    pub msg_qnum: u64,
    /// Maximum bytes allowed in queue (8 bytes).
    pub msg_qbytes: u64,
    /// PID of last `msgsnd` (4 bytes).
    pub msg_lspid: u32,
    /// PID of last `msgrcv` (4 bytes).
    pub msg_lrpid: u32,
    /// Reserved (8 bytes).
    pub __glibc_reserved4: u64,
    /// Reserved (8 bytes).
    pub __glibc_reserved5: u64,
}

impl MsqIdDs {
    pub const SIZE: usize = 120;

    /// Serializes `MsqIdDs` to a 120-byte array using zerocopy.
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        zerocopy::transmute!(*self)
    }

    /// Deserializes `MsqIdDs` from a byte slice safely.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        Self::read_from_bytes(bytes).ok()
    }
}

/// Linux compatible `struct msginfo` (32 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct MsgInfo {
    pub msgpool: i32,
    pub msgmap: i32,
    pub msgmax: i32,
    pub msgmnb: i32,
    pub msgmni: i32,
    pub msgssz: i32,
    pub msgtql: i32,
    pub msgseg: u16,
    pub __unused: u16,
}

impl Default for MsgInfo {
    fn default() -> Self {
        Self {
            msgpool: 1024,
            msgmap: 1024,
            msgmax: MSGMAX as i32,
            msgmnb: MSGMNB as i32,
            msgmni: MSGMNI as i32,
            msgssz: 16,
            msgtql: 2048,
            msgseg: 0xffff,
            __unused: 0,
        }
    }
}

impl MsgInfo {
    pub const SIZE: usize = 32;

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        zerocopy::transmute!(*self)
    }
}
