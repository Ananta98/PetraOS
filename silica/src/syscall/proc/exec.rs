// SPDX-License-Identifier: GPL-2.0

//! The `execve` and `execveat` system calls (Syscalls #59 and #322 on x86_64).

use alloc::vec::Vec;
use ostd::arch::cpu::context::UserContext;

use crate::{
    api::errno::{Errno, Result},
    proc::{
        lifecycle::{AT_EMPTY_PATH, AT_FDCWD, execve_at},
        thread::Thread,
    },
    utils::userspace::user_cstr::UserCStr,
};

/// System call `execveat(int dirfd, const char *pathname, char *const argv[], char *const envp[], int flags)` (Syscall #322).
pub fn sys_execveat(
    dirfd: i32,
    path_ptr: usize,
    argv_ptr: usize,
    envp_ptr: usize,
    flags: i32,
    ctx: &mut UserContext,
) -> Result<usize> {
    let td = Thread::current().ok_or(Errno::ESRCH)?;
    let proc = td.proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();

    let path = if path_ptr == 0 {
        if (flags & AT_EMPTY_PATH) != 0 {
            alloc::string::String::new()
        } else {
            return Err(Errno::EFAULT);
        }
    } else {
        UserCStr::new(path_ptr).read_string(&vmar, 4096)?
    };

    if path.is_empty() && (flags & AT_EMPTY_PATH) == 0 {
        return Err(Errno::ENOENT);
    }

    let argv_strings = UserCStr::read_string_array(&vmar, argv_ptr, 1024)?;
    let envp_strings = UserCStr::read_string_array(&vmar, envp_ptr, 1024)?;

    let argv_refs: Vec<&str> = argv_strings.iter().map(|s| s.as_str()).collect();
    let envp_refs: Vec<&str> = envp_strings.iter().map(|s| s.as_str()).collect();

    ostd::info!(
        "execveat: dirfd={}, path=\"{}\", argv={:?}, flags={:#x}",
        dirfd,
        path,
        argv_strings,
        flags
    );
    execve_at(&td, dirfd, &path, &argv_refs, &envp_refs, flags, ctx)?;
    Ok(0)
}

/// System call `execve(const char *pathname, char *const argv[], char *const envp[])` (Syscall #59).
pub fn sys_execve(
    path_ptr: usize,
    argv_ptr: usize,
    envp_ptr: usize,
    ctx: &mut UserContext,
) -> Result<usize> {
    sys_execveat(AT_FDCWD, path_ptr, argv_ptr, envp_ptr, 0, ctx)
}
