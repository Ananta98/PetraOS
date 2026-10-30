// SPDX-License-Identifier: GPL-2.0

//! Shared utilities and encoders/decoders for signal system calls.

use crate::uapi::signal::{SIGSET_WORDS, SigAction, SigHandler, SigSet, SigStack, Signal};

/// The size of the kernel `sigset_t` (`_NSIG / 8`), the only `sigsetsize`
/// `rt_sigaction(2)` accepts.
///
/// libc's own `sigset_t` is 128 bytes, but libc never hands that to the kernel:
/// glibc and musl both pass the 64-signal kernel mask.
pub const KERNEL_SIGSET_LEN: usize = 8;

/// The size of the kernel `struct sigaction` on x86_64.
///
/// `sa_handler` (8), `sa_flags` (8, an `unsigned long`), `sa_restorer` (8), then
/// `sa_mask` (8). This is not libc's `struct sigaction`, which puts a 128-byte
/// mask second: libc converts to this layout before the system call.
pub const SIGACTION_LEN: usize = 8 + 8 + 8 + KERNEL_SIGSET_LEN;

const SIGACTION_FLAGS_AT: usize = 8;
const SIGACTION_RESTORER_AT: usize = 16;
const SIGACTION_MASK_AT: usize = 24;

/// `sigset_t`, as passed to and from user space.
pub fn decode_sigset(buf: &[u8]) -> SigSet {
    let mut words = [0u64; SIGSET_WORDS];
    for (i, slot) in words.iter_mut().enumerate() {
        let at = i * 8;
        *slot = u64::from_le_bytes(buf[at..at + 8].try_into().expect("eight bytes"));
    }
    SigSet(words)
}

pub fn encode_sigset(set: &SigSet) -> [u8; SIGSET_WORDS * 8] {
    let mut buf = [0u8; SIGSET_WORDS * 8];
    for (i, &word) in set.0.iter().enumerate() {
        buf[i * 8..(i + 1) * 8].copy_from_slice(&word.to_le_bytes());
    }
    buf
}

fn read_u64(buf: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(buf[at..at + 8].try_into().expect("eight bytes"))
}

pub fn decode_sigaction(buf: &[u8; SIGACTION_LEN]) -> SigAction {
    let handler = read_u64(buf, 0) as usize;
    // Every defined `SA_*` flag fits in the low 32 bits of `unsigned long`.
    let flags = read_u64(buf, SIGACTION_FLAGS_AT) as u32;
    let restorer = read_u64(buf, SIGACTION_RESTORER_AT) as usize;
    let mut mask = [0u64; SIGSET_WORDS];
    mask[0] = read_u64(buf, SIGACTION_MASK_AT);

    SigAction {
        sa_handler: if handler == 0 {
            SigHandler::Default
        } else if handler == 1 {
            SigHandler::Ignore
        } else {
            SigHandler::Handler(handler)
        },
        sa_mask: SigSet(mask),
        sa_flags: flags,
        sa_restorer: restorer,
    }
}

pub fn encode_sigaction(action: &SigAction) -> [u8; SIGACTION_LEN] {
    let mut buf = [0u8; SIGACTION_LEN];
    let handler = match action.sa_handler {
        SigHandler::Default => 0usize,
        SigHandler::Ignore => 1,
        SigHandler::Handler(addr) => addr,
    };
    buf[0..8].copy_from_slice(&(handler as u64).to_le_bytes());
    buf[SIGACTION_FLAGS_AT..SIGACTION_RESTORER_AT]
        .copy_from_slice(&u64::from(action.sa_flags).to_le_bytes());
    buf[SIGACTION_RESTORER_AT..SIGACTION_MASK_AT]
        .copy_from_slice(&(action.sa_restorer as u64).to_le_bytes());
    buf[SIGACTION_MASK_AT..].copy_from_slice(&action.sa_mask.0[0].to_le_bytes());
    buf
}

pub fn decode_sigstack(buf: &[u8]) -> SigStack {
    SigStack {
        ss_sp: u64::from_le_bytes(buf[0..8].try_into().expect("eight bytes")) as usize,
        ss_flags: u32::from_le_bytes(buf[8..12].try_into().expect("four bytes")),
        _pad: 0,
        ss_size: u64::from_le_bytes(buf[16..24].try_into().expect("eight bytes")) as usize,
    }
}

pub fn encode_sigstack(stack: &SigStack) -> [u8; 24] {
    let mut buf = [0u8; 24];
    buf[0..8].copy_from_slice(&(stack.ss_sp as u64).to_le_bytes());
    buf[8..12].copy_from_slice(&stack.ss_flags.to_le_bytes());
    buf[16..24].copy_from_slice(&(stack.ss_size as u64).to_le_bytes());
    buf
}

pub fn all_signals() -> impl Iterator<Item = Signal> {
    (1..=crate::uapi::signal::SIGNAL_MAX).filter_map(Signal::from_u32)
}
