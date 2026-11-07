// SPDX-License-Identifier: GPL-2.0

//! Secure Computing (`seccomp`) UAPI constants and data structures.
//!
//! Layout and constants match Linux `include/uapi/linux/seccomp.h`,
//! `include/uapi/linux/filter.h`, and `include/uapi/linux/audit.h`.

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// Seccomp disabled for the thread.
pub const SECCOMP_MODE_DISABLED: u32 = 0;
/// Strict mode: only `read`, `write`, `_exit`, and `rt_sigreturn` are allowed.
pub const SECCOMP_MODE_STRICT: u32 = 1;
/// Filter mode: system calls are evaluated against a chain of cBPF programs.
pub const SECCOMP_MODE_FILTER: u32 = 2;

/// `seccomp(2)` operation: enter strict mode.
pub const SECCOMP_SET_MODE_STRICT: u32 = 0;
/// `seccomp(2)` operation: attach a cBPF filter program.
pub const SECCOMP_SET_MODE_FILTER: u32 = 1;
/// `seccomp(2)` operation: query whether the kernel supports a return action.
pub const SECCOMP_GET_ACTION_AVAIL: u32 = 2;
/// `seccomp(2)` operation: query sizes of user-notification structures.
pub const SECCOMP_GET_NOTIF_SIZES: u32 = 3;

/// Synchronize all threads in the caller's thread group to the same filter tree.
pub const SECCOMP_FILTER_FLAG_TSYNC: u32 = 1 << 0;
/// Log all non-`ALLOW` actions returned by this filter.
pub const SECCOMP_FILTER_FLAG_LOG: u32 = 1 << 1;
/// Disable Speculative Store Bypass mitigation for this filter.
pub const SECCOMP_FILTER_FLAG_SPEC_ALLOW: u32 = 1 << 2;
/// Create a new user-notification listener file descriptor.
pub const SECCOMP_FILTER_FLAG_NEW_LISTENER: u32 = 1 << 3;
/// Return `-ESRCH` instead of the offending TID on `TSYNC` failure.
pub const SECCOMP_FILTER_FLAG_TSYNC_ESRCH: u32 = 1 << 4;

/// Mask of all supported `SECCOMP_SET_MODE_FILTER` flags.
pub const SECCOMP_FILTER_FLAG_MASK: u32 = SECCOMP_FILTER_FLAG_TSYNC
    | SECCOMP_FILTER_FLAG_LOG
    | SECCOMP_FILTER_FLAG_SPEC_ALLOW
    | SECCOMP_FILTER_FLAG_TSYNC_ESRCH;

/// Kill the entire process immediately with `SIGSYS`.
pub const SECCOMP_RET_KILL_PROCESS: u32 = 0x8000_0000;
/// Kill only the calling thread immediately with `SIGSYS`.
pub const SECCOMP_RET_KILL_THREAD: u32 = 0x0000_0000;
/// Alias for `SECCOMP_RET_KILL_THREAD`.
pub const SECCOMP_RET_KILL: u32 = SECCOMP_RET_KILL_THREAD;
/// Disallow syscall and deliver synchronous `SIGSYS` (`SYS_SECCOMP`).
pub const SECCOMP_RET_TRAP: u32 = 0x0003_0000;
/// Disallow syscall and return the 16-bit data field as negated `errno`.
pub const SECCOMP_RET_ERRNO: u32 = 0x0005_0000;
/// Forward syscall to a userspace notifier listener.
pub const SECCOMP_RET_USER_NOTIF: u32 = 0x7fc0_0000;
/// Notify a `ptrace` tracer or fail with `-ENOSYS` if no tracer is attached.
pub const SECCOMP_RET_TRACE: u32 = 0x7ff0_0000;
/// Allow the syscall after logging its execution.
pub const SECCOMP_RET_LOG: u32 = 0x7ffc_0000;
/// Allow the syscall to execute normally.
pub const SECCOMP_RET_ALLOW: u32 = 0x7fff_0000;

/// Bitmask extracting the full return action (including `KILL_PROCESS` bit).
pub const SECCOMP_RET_ACTION_FULL: u32 = 0xffff_0000;
/// Bitmask extracting the precedence-ordered action bits.
pub const SECCOMP_RET_ACTION: u32 = 0x7fff_0000;
/// Bitmask extracting the 16-bit action payload (`errno`, trap data, trace data).
pub const SECCOMP_RET_DATA: u32 = 0x0000_ffff;

/// Linux audit architecture identifier for `x86_64` (`EM_X86_64 | __AUDIT_ARCH_64BIT | __AUDIT_ARCH_LE`).
pub const AUDIT_ARCH_X86_64: u32 = 0xc000_003e;

/// Maximum number of instructions permitted in a single cBPF program.
pub const BPF_MAXINSNS: usize = 4096;
/// Number of 32-bit scratch memory words in the cBPF virtual machine.
pub const BPF_MEMWORDS: usize = 16;

// Classic BPF instruction classes (bits 0..=2).
pub const BPF_LD: u16 = 0x00;
pub const BPF_LDX: u16 = 0x01;
pub const BPF_ST: u16 = 0x02;
pub const BPF_STX: u16 = 0x03;
pub const BPF_ALU: u16 = 0x04;
pub const BPF_JMP: u16 = 0x05;
pub const BPF_RET: u16 = 0x06;
pub const BPF_MISC: u16 = 0x07;

// Classic BPF load/store size and addressing modes.
pub const BPF_W: u16 = 0x00;
pub const BPF_H: u16 = 0x08;
pub const BPF_B: u16 = 0x10;
pub const BPF_IMM: u16 = 0x00;
pub const BPF_ABS: u16 = 0x20;
pub const BPF_IND: u16 = 0x40;
pub const BPF_MEM: u16 = 0x60;
pub const BPF_LEN: u16 = 0x80;
pub const BPF_MSH: u16 = 0xa0;

// Classic BPF ALU and jump operations (bits 4..=7).
pub const BPF_ADD: u16 = 0x00;
pub const BPF_SUB: u16 = 0x10;
pub const BPF_MUL: u16 = 0x20;
pub const BPF_DIV: u16 = 0x30;
pub const BPF_OR: u16 = 0x40;
pub const BPF_AND: u16 = 0x50;
pub const BPF_LSH: u16 = 0x60;
pub const BPF_RSH: u16 = 0x70;
pub const BPF_NEG: u16 = 0x80;
pub const BPF_MOD: u16 = 0x90;
pub const BPF_XOR: u16 = 0xa0;

pub const BPF_JA: u16 = 0x00;
pub const BPF_JEQ: u16 = 0x10;
pub const BPF_JGT: u16 = 0x20;
pub const BPF_JGE: u16 = 0x30;
pub const BPF_JSET: u16 = 0x40;

// Classic BPF source operands.
pub const BPF_K: u16 = 0x00;
pub const BPF_X: u16 = 0x08;
pub const BPF_A: u16 = 0x10;

// Classic BPF miscellaneous operations.
pub const BPF_TAX: u16 = 0x00;
pub const BPF_TXA: u16 = 0x80;

/// Classic BPF instruction (`struct sock_filter`, 8 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct SockFilter {
    /// Raw filter opcode.
    pub code: u16,
    /// Jump offset if condition is true.
    pub jt: u8,
    /// Jump offset if condition is false.
    pub jf: u8,
    /// Generic 32-bit multiuse constant field.
    pub k: u32,
}

/// Classic BPF program header (`struct sock_fprog`, 16 bytes on 64-bit).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct SockFprog {
    /// Number of `SockFilter` instructions at `filter`.
    pub len: u16,
    pub _pad: [u16; 3],
    /// Userspace pointer to array of `SockFilter`.
    pub filter: usize,
}

/// System call context evaluated by seccomp cBPF filters (`struct seccomp_data`, 64 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct SeccompData {
    /// System call number.
    pub nr: i32,
    /// Architecture identifier (`AUDIT_ARCH_X86_64`).
    pub arch: u32,
    /// Userspace instruction pointer (`RIP`) at the time of the syscall.
    pub instruction_pointer: u64,
    /// System call arguments (`rdi`, `rsi`, `rdx`, `r10`, `r8`, `r9`).
    pub args: [u64; 6],
}

impl SeccompData {
    /// Size of `struct seccomp_data` in bytes.
    pub const LEN: usize = core::mem::size_of::<Self>();

    /// Reads a 32-bit word at the given 4-byte aligned byte offset within `SeccompData`.
    pub const fn read_u32(&self, offset: u32) -> Option<u32> {
        match offset {
            0 => Some(self.nr as u32),
            4 => Some(self.arch),
            8 => Some(self.instruction_pointer as u32),
            12 => Some((self.instruction_pointer >> 32) as u32),
            16 => Some(self.args[0] as u32),
            20 => Some((self.args[0] >> 32) as u32),
            24 => Some(self.args[1] as u32),
            28 => Some((self.args[1] >> 32) as u32),
            32 => Some(self.args[2] as u32),
            36 => Some((self.args[2] >> 32) as u32),
            40 => Some(self.args[3] as u32),
            44 => Some((self.args[3] >> 32) as u32),
            48 => Some(self.args[4] as u32),
            52 => Some((self.args[4] >> 32) as u32),
            56 => Some(self.args[5] as u32),
            60 => Some((self.args[5] >> 32) as u32),
            _ => None,
        }
    }
}

/// Sizes of seccomp user-notification structures (`struct seccomp_notif_sizes`, 6 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct SeccompNotifSizes {
    pub seccomp_notif: u16,
    pub seccomp_notif_resp: u16,
    pub seccomp_data: u16,
}

impl SeccompNotifSizes {
    /// Standard Linux x86_64 sizes (`seccomp_notif = 80`, `seccomp_notif_resp = 24`, `seccomp_data = 64`).
    pub const fn standard() -> Self {
        Self {
            seccomp_notif: 80,
            seccomp_notif_resp: 24,
            seccomp_data: SeccompData::LEN as u16,
        }
    }
}
