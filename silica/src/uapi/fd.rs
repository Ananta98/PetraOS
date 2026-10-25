// SPDX-License-Identifier: GPL-2.0

//! UAPI definitions and constants for file descriptors.
//!
//! Includes POSIX / Linux ABI flags, seals, and structures for:
//! - File descriptor flags (`FD_CLOEXEC`)
//! - Memory-backed file descriptors (`memfd_create`)
//! - Event notification descriptors (`eventfd`, `eventfd2`)
//! - Timer notification descriptors (`timerfd_create`, `timerfd_settime`, `timerfd_gettime`)
//! - Signal notification descriptors (`signalfd`, `signalfd4`)

use crate::uapi::signal::{SIGSET_WORDS, SigInfo};

// --- General File Descriptor Flags ---
pub const FD_CLOEXEC: u32 = 1;

// --- memfd_create(2) flags & seals ---
pub const MFD_CLOEXEC: u32 = 0x0001;
pub const MFD_ALLOW_SEALING: u32 = 0x0002;
pub const MFD_HUGETLB: u32 = 0x0004;
pub const MFD_HUGE_SHIFT: u32 = 26;
pub const MFD_HUGE_MASK: u32 = 0x3f << MFD_HUGE_SHIFT;
pub const MFD_KNOWN_FLAGS: u32 = MFD_CLOEXEC | MFD_ALLOW_SEALING | MFD_HUGETLB | MFD_HUGE_MASK;
pub const MFD_NAME_MAX_LEN: usize = 249;

pub const F_SEAL_SEAL: u32 = 0x0001;
pub const F_SEAL_SHRINK: u32 = 0x0002;
pub const F_SEAL_GROW: u32 = 0x0004;
pub const F_SEAL_WRITE: u32 = 0x0008;
pub const F_SEAL_FUTURE_WRITE: u32 = 0x0010;

// --- eventfd(2) / eventfd2(2) flags ---
pub const EFD_SEMAPHORE: i32 = 1;
pub const EFD_CLOEXEC: i32 = 0o2000000;
pub const EFD_NONBLOCK: i32 = 0o4000;
pub const EFD_VALID_FLAGS_MASK: i32 = EFD_SEMAPHORE | EFD_CLOEXEC | EFD_NONBLOCK;

pub const EVENTFD_MAX_COUNTER: u64 = 0xFFFF_FFFF_FFFF_FFFE;
pub const EVENTFD_PAYLOAD_SIZE: usize = 8;

// --- timerfd_create(2) / timerfd_settime(2) flags ---
pub const TFD_CLOEXEC: i32 = 0o2000000;
pub const TFD_NONBLOCK: i32 = 0o4000;
pub const TFD_CREATE_FLAGS_MASK: i32 = TFD_CLOEXEC | TFD_NONBLOCK;

pub const TFD_TIMER_ABSTIME: i32 = 1;
pub const TFD_TIMER_CANCEL_ON_SET: i32 = 2;
pub const TFD_SETTIME_FLAGS_MASK: i32 = TFD_TIMER_ABSTIME | TFD_TIMER_CANCEL_ON_SET;

pub const TIMERFD_PAYLOAD_SIZE: usize = 8;
pub const ITIMERSPEC_RAW_SIZE: usize = 32;
pub const NANOS_PER_SEC: u64 = 1_000_000_000;

// --- signalfd(2) / signalfd4(2) flags & struct ---
pub const SFD_CLOEXEC: i32 = 0o2000000;
pub const SFD_NONBLOCK: i32 = 0o4000;
pub const SFD_VALID_FLAGS_MASK: i32 = SFD_CLOEXEC | SFD_NONBLOCK;

pub const SIGNALFD_SIGINFO_SIZE: usize = 128;
pub const SIGSET_ABI_SIZE_MIN: usize = 8;
pub const BYTES_PER_SIGSET_WORD: usize = 8;
pub const SIGSET_ABI_SIZE_MAX: usize = SIGSET_WORDS * BYTES_PER_SIGSET_WORD;

/// Linux ABI layout for `struct signalfd_siginfo` (128 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SignalFdSiginfo {
    pub ssi_signo: u32,
    pub ssi_errno: i32,
    pub ssi_code: i32,
    pub ssi_pid: u32,
    pub ssi_uid: u32,
    pub ssi_fd: i32,
    pub ssi_tid: u32,
    pub ssi_band: u32,
    pub ssi_overrun: u32,
    pub ssi_trapno: u32,
    pub ssi_status: i32,
    pub ssi_int: i32,
    pub ssi_ptr: u64,
    pub ssi_utime: u64,
    pub ssi_stime: u64,
    pub ssi_addr: u64,
    pub ssi_addr_lsb: u16,
    pub _pad2: u16,
    pub ssi_syscall: i32,
    pub ssi_call_addr: u64,
    pub ssi_arch: u32,
    pub _pad: [u8; 28],
}

impl SignalFdSiginfo {
    pub fn from_siginfo(info: &SigInfo) -> Self {
        Self {
            ssi_signo: info.si_signo as u32,
            ssi_errno: info.si_errno,
            ssi_code: info.si_code,
            ssi_pid: info.si_pid as u32,
            ssi_uid: info.si_uid,
            ssi_status: info.si_status,
            ssi_addr: info.si_addr,
            ssi_ptr: info.si_value,
            ..Default::default()
        }
    }

    pub fn to_bytes(&self) -> [u8; SIGNALFD_SIGINFO_SIZE] {
        let mut buffer = [0u8; SIGNALFD_SIGINFO_SIZE];
        buffer[0..4].copy_from_slice(&self.ssi_signo.to_ne_bytes());
        buffer[4..8].copy_from_slice(&self.ssi_errno.to_ne_bytes());
        buffer[8..12].copy_from_slice(&self.ssi_code.to_ne_bytes());
        buffer[12..16].copy_from_slice(&self.ssi_pid.to_ne_bytes());
        buffer[16..20].copy_from_slice(&self.ssi_uid.to_ne_bytes());
        buffer[20..24].copy_from_slice(&self.ssi_fd.to_ne_bytes());
        buffer[24..28].copy_from_slice(&self.ssi_tid.to_ne_bytes());
        buffer[28..32].copy_from_slice(&self.ssi_band.to_ne_bytes());
        buffer[32..36].copy_from_slice(&self.ssi_overrun.to_ne_bytes());
        buffer[36..40].copy_from_slice(&self.ssi_trapno.to_ne_bytes());
        buffer[40..44].copy_from_slice(&self.ssi_status.to_ne_bytes());
        buffer[44..48].copy_from_slice(&self.ssi_int.to_ne_bytes());
        buffer[48..56].copy_from_slice(&self.ssi_ptr.to_ne_bytes());
        buffer[56..64].copy_from_slice(&self.ssi_utime.to_ne_bytes());
        buffer[64..72].copy_from_slice(&self.ssi_stime.to_ne_bytes());
        buffer[72..80].copy_from_slice(&self.ssi_addr.to_ne_bytes());
        buffer[80..82].copy_from_slice(&self.ssi_addr_lsb.to_ne_bytes());
        buffer[82..84].copy_from_slice(&self._pad2.to_ne_bytes());
        buffer[84..88].copy_from_slice(&self.ssi_syscall.to_ne_bytes());
        buffer[88..96].copy_from_slice(&self.ssi_call_addr.to_ne_bytes());
        buffer[96..100].copy_from_slice(&self.ssi_arch.to_ne_bytes());
        buffer[100..128].copy_from_slice(&self._pad);
        buffer
    }
}
