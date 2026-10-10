// SPDX-License-Identifier: GPL-2.0

//! Shared utilities and encoders/decoders for signal system calls.

use crate::api::signal::{SIGSET_WORDS, SigAction, SigHandler, SigSet, SigStack, Signal};

/// The size of `struct sigaction` as user space lays it out.
///
/// A handler (8), a 128-byte mask (SIGSET_WORDS * 8), flags (4), pad (4), restorer (8).
pub const SIGACTION_LEN: usize = 8 + SIGSET_WORDS * 8 + 4 + 4 + 8;

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

pub fn decode_sigaction(buf: &[u8]) -> SigAction {
    let mut mask = [0u64; SIGSET_WORDS];
    for (i, slot) in mask.iter_mut().enumerate() {
        let at = 8 + i * 8;
        *slot = u64::from_le_bytes(buf[at..at + 8].try_into().expect("eight bytes"));
    }
    let handler = u64::from_le_bytes(buf[0..8].try_into().expect("eight bytes")) as usize;
    let flags = u32::from_le_bytes(buf[8 + SIGSET_WORDS * 8..][..4].try_into().expect("four bytes"));
    let restorer_at = 8 + SIGSET_WORDS * 8 + 8;
    let restorer =
        u64::from_le_bytes(buf[restorer_at..][..8].try_into().expect("eight bytes")) as usize;

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
    buf[8..8 + SIGSET_WORDS * 8].copy_from_slice(&encode_sigset(&action.sa_mask));
    let flags_at = 8 + SIGSET_WORDS * 8;
    buf[flags_at..flags_at + 4].copy_from_slice(&action.sa_flags.to_le_bytes());
    buf[flags_at + 8..flags_at + 16].copy_from_slice(&(action.sa_restorer as u64).to_le_bytes());
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
    (1..=crate::api::signal::SIGNAL_MAX).filter_map(Signal::from_u32)
}

pub fn read_bytes(vmar: &crate::vm::vmar::Vmar, uaddr: usize, len: usize) -> crate::api::errno::Result<alloc::vec::Vec<u8>> {
    use ostd::mm::FallibleVmRead;
    let mut buf = alloc::vec![0u8; len];
    let mut reader = vmar.vm_space().reader(uaddr, len).map_err(|_| crate::api::errno::Errno::EFAULT)?;
    let mut writer = ostd::mm::VmWriter::from(&mut buf[..]);
    reader.read_fallible(&mut writer).map_err(|_| crate::api::errno::Errno::EFAULT)?;
    Ok(buf)
}

pub fn write_bytes(vmar: &crate::vm::vmar::Vmar, uaddr: usize, bytes: &[u8]) -> crate::api::errno::Result<()> {
    use ostd::mm::FallibleVmWrite;
    let mut writer = vmar.vm_space().writer(uaddr, bytes.len()).map_err(|_| crate::api::errno::Errno::EFAULT)?;
    let mut reader = ostd::mm::VmReader::from(bytes);
    writer.write_fallible(&mut reader).map_err(|_| crate::api::errno::Errno::EFAULT)?;
    Ok(())
}
