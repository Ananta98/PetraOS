// SPDX-License-Identifier: GPL-2.0

//! `mincore(2)` system call (Syscall #27 on x86_64).

use ostd::arch::cpu::context::UserContext;

use crate::{
    proc::thread::Thread,
    uapi::errno::{Errno, Result},
    utils::UserPtr,
};

/// System call `mincore(addr, len, vec)` (Syscall #27 on x86_64).
pub fn sys_mincore(
    addr: usize,
    len: usize,
    vec_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if !addr.is_multiple_of(ostd::mm::PAGE_SIZE) {
        return Err(Errno::EINVAL);
    }
    if len == 0 {
        return Ok(0);
    }
    let Some(page_len) = len.checked_next_multiple_of(ostd::mm::PAGE_SIZE) else {
        crate::return_errno!(ENOMEM, "the length overflows");
    };
    let end = addr.checked_add(page_len).ok_or(Errno::ENOMEM)?;
    if end > ostd::mm::MAX_USERSPACE_VADDR {
        return Err(Errno::ENOMEM);
    }

    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();

    // Process in bounded 4 KiB chunks to prevent unbounded kernel heap allocation.
    const CHUNK_PAGES: usize = 4096;
    let total_pages = page_len / ostd::mm::PAGE_SIZE;
    let mut chunk_buf = alloc::vec![0u8; total_pages.min(CHUNK_PAGES)];
    let mut page_offset = 0usize;

    while page_offset < total_pages {
        let count = (total_pages - page_offset).min(CHUNK_PAGES);
        let chunk_start = addr + page_offset * ostd::mm::PAGE_SIZE;
        let chunk_end = chunk_start + count * ostd::mm::PAGE_SIZE;
        let out_ptr = vec_ptr.checked_add(page_offset).ok_or(Errno::EFAULT)?;

        vmar.mincore(chunk_start..chunk_end, &mut chunk_buf[..count])?;
        UserPtr::new(out_ptr).write_bytes(&vmar, &chunk_buf[..count])?;
        page_offset += count;
    }

    Ok(0)
}
