// SPDX-License-Identifier: GPL-2.0

//! System call `memfd_create`.

use ostd::arch::cpu::context::UserContext;

use crate::{
    fs::{create_memfd_file, fd::FdFlags},
    proc::thread::Thread,
    syscall::fs::utils::UserCStr,
    uapi::{
        errno::{Errno, Result},
        fd::{MFD_CLOEXEC, MFD_KNOWN_FLAGS, MFD_NAME_MAX_LEN},
    },
};

/// System call `memfd_create(const char *name, unsigned int flags)` (Syscall #319).
pub fn sys_memfd_create(name_ptr: usize, flags: u32, _ctx: &mut UserContext) -> Result<usize> {
    if (flags & !MFD_KNOWN_FLAGS) != 0 {
        return Err(Errno::EINVAL);
    }
    if name_ptr == 0 {
        return Err(Errno::EFAULT);
    }

    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();
    let name = UserCStr::new(name_ptr).read_string(&vmar, MFD_NAME_MAX_LEN + 1)?;
    if name.len() > MFD_NAME_MAX_LEN {
        return Err(Errno::EINVAL);
    }

    let file = create_memfd_file(name, flags)?;
    let mut descriptor_flags = FdFlags::empty();
    if (flags & MFD_CLOEXEC) != 0 {
        descriptor_flags |= FdFlags::FD_CLOEXEC;
    }

    let new_fd = proc.fd_table.lock().alloc_fd(file, descriptor_flags)?;
    Ok(new_fd as usize)
}
