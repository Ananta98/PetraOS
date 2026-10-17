// SPDX-License-Identifier: GPL-2.0

//! The `execve` system call (Syscall #59 on x86_64).

use alloc::vec::Vec;
use ostd::arch::cpu::context::UserContext;

use crate::{
    api::errno::{Errno, Result},
    proc::{lifecycle::execve, thread::Thread},
    utils::userspace::user_cstr::UserCStr,
};

/// System call `execve(const char *pathname, char *const argv[], char *const envp[])` (Syscall #59).
pub fn sys_execve(
    path_ptr: usize,
    argv_ptr: usize,
    envp_ptr: usize,
    ctx: &mut UserContext,
) -> Result<usize> {
    let td = Thread::current().ok_or(Errno::ESRCH)?;
    let proc = td.proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();

    let path = UserCStr::new(path_ptr).read_string(&vmar, 4096)?;
    let argv_strings = UserCStr::read_string_array(&vmar, argv_ptr, 1024)?;
    let envp_strings = UserCStr::read_string_array(&vmar, envp_ptr, 1024)?;

    let argv_refs: Vec<&str> = argv_strings.iter().map(|s| s.as_str()).collect();
    let envp_refs: Vec<&str> = envp_strings.iter().map(|s| s.as_str()).collect();

    ostd::info!("execve: path=\"{}\", argv={:?}", path, argv_strings);
    execve(&td, &path, &argv_refs, &envp_refs, ctx)?;
    Ok(0)
}
