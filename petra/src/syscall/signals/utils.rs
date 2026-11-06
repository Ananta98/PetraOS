// SPDX-License-Identifier: GPL-2.0

//! Shared utilities and encoders/decoders for signal system calls.

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

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

/// Kernel `struct sigaction` layout (32 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct KernelSigAction {
    pub sa_handler: u64,
    pub sa_flags: u64,
    pub sa_restorer: u64,
    pub sa_mask: u64,
}

/// `sigset_t`, as passed to and from user space.
pub fn decode_sigset(buf: &[u8]) -> SigSet {
    SigSet::read_from_bytes(buf).unwrap_or_default()
}

pub fn encode_sigset(set: &SigSet) -> [u8; SIGSET_WORDS * 8] {
    zerocopy::transmute!(*set)
}

pub fn decode_sigaction(buf: &[u8; SIGACTION_LEN]) -> SigAction {
    let raw = KernelSigAction::read_from_bytes(buf).unwrap_or_default();
    let handler = raw.sa_handler as usize;
    let flags = raw.sa_flags as u32;
    let restorer = raw.sa_restorer as usize;
    let mut mask = [0u64; SIGSET_WORDS];
    mask[0] = raw.sa_mask;

    SigAction {
        sa_handler: match handler {
            0 => SigHandler::Default,
            1 => SigHandler::Ignore,
            addr => SigHandler::Handler(addr),
        },
        sa_mask: SigSet(mask),
        sa_flags: flags,
        sa_restorer: restorer,
    }
}

pub fn encode_sigaction(action: &SigAction) -> [u8; SIGACTION_LEN] {
    let handler = match action.sa_handler {
        SigHandler::Default => 0u64,
        SigHandler::Ignore => 1,
        SigHandler::Handler(addr) => addr as u64,
    };
    let raw = KernelSigAction {
        sa_handler: handler,
        sa_flags: u64::from(action.sa_flags),
        sa_restorer: action.sa_restorer as u64,
        sa_mask: action.sa_mask.0[0],
    };
    zerocopy::transmute!(raw)
}

pub fn decode_sigstack(buf: &[u8]) -> SigStack {
    SigStack::read_from_bytes(buf).unwrap_or_default()
}

pub fn encode_sigstack(stack: &SigStack) -> [u8; 24] {
    zerocopy::transmute!(*stack)
}

pub fn all_signals() -> impl Iterator<Item = Signal> {
    (1..=crate::uapi::signal::SIGNAL_MAX).filter_map(Signal::from_u32)
}
