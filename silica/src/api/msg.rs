// SPDX-License-Identifier: GPL-2.0

//! System V Message Queue ABI definitions.
//!
//! Matches Linux x86-64 `struct msqid_ds` (120 bytes) and `struct msginfo`.

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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

    /// Serializes `MsqIdDs` to a 120-byte array without unsafe code.
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut buf = [0u8; Self::SIZE];
        buf[0..48].copy_from_slice(&self.msg_perm.to_bytes());
        buf[48..56].copy_from_slice(&self.msg_stime.to_ne_bytes());
        buf[56..64].copy_from_slice(&self.msg_rtime.to_ne_bytes());
        buf[64..72].copy_from_slice(&self.msg_ctime.to_ne_bytes());
        buf[72..80].copy_from_slice(&self.msg_cbytes.to_ne_bytes());
        buf[80..88].copy_from_slice(&self.msg_qnum.to_ne_bytes());
        buf[88..96].copy_from_slice(&self.msg_qbytes.to_ne_bytes());
        buf[96..100].copy_from_slice(&self.msg_lspid.to_ne_bytes());
        buf[100..104].copy_from_slice(&self.msg_lrpid.to_ne_bytes());
        buf[104..112].copy_from_slice(&self.__glibc_reserved4.to_ne_bytes());
        buf[112..120].copy_from_slice(&self.__glibc_reserved5.to_ne_bytes());
        buf
    }

    /// Deserializes `MsqIdDs` from a byte slice safely.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < Self::SIZE {
            return None;
        }
        let msg_perm = IpcPerm::from_bytes(&bytes[0..48])?;
        let msg_stime = i64::from_ne_bytes(bytes[48..56].try_into().ok()?);
        let msg_rtime = i64::from_ne_bytes(bytes[56..64].try_into().ok()?);
        let msg_ctime = i64::from_ne_bytes(bytes[64..72].try_into().ok()?);
        let msg_cbytes = u64::from_ne_bytes(bytes[72..80].try_into().ok()?);
        let msg_qnum = u64::from_ne_bytes(bytes[80..88].try_into().ok()?);
        let msg_qbytes = u64::from_ne_bytes(bytes[88..96].try_into().ok()?);
        let msg_lspid = u32::from_ne_bytes(bytes[96..100].try_into().ok()?);
        let msg_lrpid = u32::from_ne_bytes(bytes[100..104].try_into().ok()?);
        let __glibc_reserved4 = u64::from_ne_bytes(bytes[104..112].try_into().ok()?);
        let __glibc_reserved5 = u64::from_ne_bytes(bytes[112..120].try_into().ok()?);

        Some(Self {
            msg_perm,
            msg_stime,
            msg_rtime,
            msg_ctime,
            msg_cbytes,
            msg_qnum,
            msg_qbytes,
            msg_lspid,
            msg_lrpid,
            __glibc_reserved4,
            __glibc_reserved5,
        })
    }
}

/// Linux compatible `struct msginfo` (32 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
        let mut buf = [0u8; Self::SIZE];
        buf[0..4].copy_from_slice(&self.msgpool.to_ne_bytes());
        buf[4..8].copy_from_slice(&self.msgmap.to_ne_bytes());
        buf[8..12].copy_from_slice(&self.msgmax.to_ne_bytes());
        buf[12..16].copy_from_slice(&self.msgmnb.to_ne_bytes());
        buf[16..20].copy_from_slice(&self.msgmni.to_ne_bytes());
        buf[20..24].copy_from_slice(&self.msgssz.to_ne_bytes());
        buf[24..28].copy_from_slice(&self.msgtql.to_ne_bytes());
        buf[28..30].copy_from_slice(&self.msgseg.to_ne_bytes());
        buf[30..32].copy_from_slice(&self.__unused.to_ne_bytes());
        buf
    }
}
