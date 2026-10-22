// SPDX-License-Identifier: GPL-2.0

//! Loadable Kernel Module (LKM) system calls.
//!
//! Provides Linux-compatible `init_module(2)` (175), `delete_module(2)` (176),
//! and `finit_module(2)` (313).

use alloc::{string::String, vec};
use ostd::arch::cpu::context::UserContext;

use crate::{
    uapi::errno::{Errno, Result},
    proc::thread::Thread,
    utils::userspace::{UserCStr, UserPtr},
};

/// Handles `init_module(2)` system call (`SYS_INIT_MODULE = 175`).
///
/// Loads an ELF kernel module image directly from userspace memory.
pub fn sys_init_module(
    module_image: usize,
    len: usize,
    param_values: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    if !proc.cred().is_superuser() {
        return Err(Errno::EPERM);
    }
    if module_image == 0 || len == 0 || len > 64 * 1024 * 1024 {
        return Err(Errno::EINVAL);
    }

    let vmar = proc.vmspace();
    let mut elf_data = vec![0u8; len];
    UserPtr::new(module_image).read_bytes(&vmar, &mut elf_data)?;

    let cmdline = if param_values == 0 {
        String::new()
    } else {
        UserCStr::new(param_values).read_string(&vmar, 1024)?
    };

    let _name = crate::modules::load_module(&elf_data, &cmdline)?;
    Ok(0)
}

/// Handles `finit_module(2)` system call (`SYS_FINIT_MODULE = 313`).
///
/// Loads an ELF kernel module image from an open file descriptor.
pub fn sys_finit_module(
    fd: i32,
    param_values: usize,
    _flags: i32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    if !proc.cred().is_superuser() {
        return Err(Errno::EPERM);
    }

    let file = proc.file(fd)?;
    let size = file.inode.size() as usize;
    if size == 0 || size > 64 * 1024 * 1024 {
        return Err(Errno::EINVAL);
    }

    let mut elf_data = vec![0u8; size];
    let nread = file.ops.read(0, &mut elf_data)?;
    if nread == 0 {
        return Err(Errno::ENOEXEC);
    }
    elf_data.truncate(nread);

    let vmar = proc.vmspace();
    let cmdline = if param_values == 0 {
        String::new()
    } else {
        UserCStr::new(param_values).read_string(&vmar, 1024)?
    };

    let _name = crate::modules::load_module(&elf_data, &cmdline)?;
    Ok(0)
}

/// Handles `delete_module(2)` system call (`SYS_DELETE_MODULE = 176`).
///
/// Unloads a kernel module by name.
pub fn sys_delete_module(name_ptr: usize, _flags: i32, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    if !proc.cred().is_superuser() {
        return Err(Errno::EPERM);
    }

    let vmar = proc.vmspace();
    let name = UserCStr::new(name_ptr).read_string(&vmar, 256)?;
    crate::modules::unload_module(&name)?;
    Ok(0)
}
