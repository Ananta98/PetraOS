// SPDX-License-Identifier: GPL-2.0

//! System calls for signalfd: `signalfd`, `signalfd4`.

use alloc::sync::Arc;
use ostd::arch::cpu::context::UserContext;

use crate::{
    fs::{create_signalfd_file, fd::FdFlags},
    proc::thread::Thread,
    syscall::fs::utils::UserPtr,
    uapi::{
        errno::{Errno, Result},
        fd::{
            BYTES_PER_SIGSET_WORD, SFD_CLOEXEC, SFD_VALID_FLAGS_MASK, SIGSET_ABI_SIZE_MAX,
            SIGSET_ABI_SIZE_MIN,
        },
        signal::{SIGSET_WORDS, SigSet},
    },
};

fn decode_user_sigset(
    mask_ptr: usize,
    sizemask: usize,
    vmar: &Arc<crate::vm::Vmar>,
) -> Result<SigSet> {
    if sizemask != SIGSET_ABI_SIZE_MIN && sizemask != SIGSET_ABI_SIZE_MAX {
        return Err(Errno::EINVAL);
    }
    let mut words = [0u64; SIGSET_WORDS];
    let copy_bytes = sizemask.min(SIGSET_ABI_SIZE_MAX);
    let mut raw_bytes = [0u8; SIGSET_ABI_SIZE_MAX];
    UserPtr::new(mask_ptr).read_bytes(vmar, &mut raw_bytes[..copy_bytes])?;

    for (index, slot) in words.iter_mut().enumerate() {
        let offset = index * BYTES_PER_SIGSET_WORD;
        if offset + BYTES_PER_SIGSET_WORD <= copy_bytes {
            *slot = u64::from_le_bytes(
                raw_bytes[offset..offset + BYTES_PER_SIGSET_WORD]
                    .try_into()
                    .unwrap(),
            );
        }
    }
    Ok(SigSet(words))
}

/// System call `signalfd(int fd, const sigset_t *mask, size_t sizemask)` (Syscall #282).
pub fn sys_signalfd(
    fd: i32,
    mask_ptr: usize,
    sizemask: usize,
    ctx: &mut UserContext,
) -> Result<usize> {
    sys_signalfd4(fd, mask_ptr, sizemask, 0, ctx)
}

/// System call `signalfd4(int fd, const sigset_t *mask, size_t sizemask, int flags)` (Syscall #289).
pub fn sys_signalfd4(
    fd: i32,
    mask_ptr: usize,
    sizemask: usize,
    flags: i32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if (flags & !SFD_VALID_FLAGS_MASK) != 0 || mask_ptr == 0 {
        return Err(Errno::EINVAL);
    }

    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();
    let mask = decode_user_sigset(mask_ptr, sizemask, &vmar)?;

    if fd >= 0 {
        let target_file = proc.file(fd)?;
        let signalfd = target_file.as_signalfd().ok_or(Errno::EINVAL)?;
        signalfd.set_mask(mask);
        return Ok(fd as usize);
    }

    if fd != -1 {
        return Err(Errno::EBADF);
    }

    let file = create_signalfd_file(mask, flags);
    let mut descriptor_flags = FdFlags::empty();
    if (flags & SFD_CLOEXEC) != 0 {
        descriptor_flags |= FdFlags::FD_CLOEXEC;
    }

    let new_fd = proc.fd_table.lock().alloc_fd(file, descriptor_flags)?;
    Ok(new_fd as usize)
}
