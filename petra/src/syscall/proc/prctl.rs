// SPDX-License-Identifier: GPL-2.0

//! `prctl(2)` system call (Syscall #157 on x86_64).

use alloc::vec::Vec;
use ostd::arch::cpu::context::UserContext;

use crate::{
    proc::{
        Thread,
        seccomp::{BPF_MAXINSNS, set_mode_filter, set_mode_strict},
    },
    uapi::{
        errno::{Errno, Result},
        seccomp::{SECCOMP_MODE_FILTER, SECCOMP_MODE_STRICT, SockFilter, SockFprog},
    },
    utils::UserPtr,
};

const PR_SET_PDEATHSIG: i32 = 1;
const PR_GET_PDEATHSIG: i32 = 2;
const PR_GET_DUMPABLE: i32 = 3;
const PR_SET_DUMPABLE: i32 = 4;
const PR_GET_KEEPCAPS: i32 = 7;
const PR_SET_KEEPCAPS: i32 = 8;
const PR_SET_NAME: i32 = 15;
const PR_GET_NAME: i32 = 16;
const PR_GET_SECCOMP: i32 = 21;
const PR_SET_SECCOMP: i32 = 22;
const PR_CAPBSET_DROP: i32 = 24;
const PR_GET_SECUREBITS: i32 = 27;
const PR_SET_SECUREBITS: i32 = 28;
const PR_SET_CHILD_SUBREAPER: i32 = 36;
const PR_GET_CHILD_SUBREAPER: i32 = 37;
const PR_SET_NO_NEW_PRIVS: i32 = 38;
const PR_GET_NO_NEW_PRIVS: i32 = 39;

const COMM_LEN: usize = 16;

pub const ARCH_SET_GS: i32 = 0x1001;
pub const ARCH_SET_FS: i32 = 0x1002;
pub const ARCH_GET_FS: i32 = 0x1003;
pub const ARCH_GET_GS: i32 = 0x1004;

/// `prctl(2)` - operations on a process or thread.
pub fn sys_prctl(
    option: i32,
    arg2: usize,
    arg3: usize,
    arg4: usize,
    arg5: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();

    match option {
        PR_SET_NAME => {
            if arg2 == 0 {
                return Err(Errno::EFAULT);
            }
            let mut name_buf = [0u8; COMM_LEN];
            UserPtr::new(arg2).read_bytes(&vmar, &mut name_buf)?;

            // Ensure null termination at the end
            name_buf[COMM_LEN - 1] = 0;

            let mut inner = proc.inner.lock();
            inner.comm = name_buf;
            Ok(0)
        }
        PR_GET_NAME => {
            if arg2 == 0 {
                return Err(Errno::EFAULT);
            }
            let comm = proc.inner.lock().comm;
            UserPtr::new(arg2).write_bytes(&vmar, &comm)?;
            Ok(0)
        }
        PR_GET_DUMPABLE => Ok(1),
        PR_SET_DUMPABLE => {
            if arg2 > 1 {
                return Err(Errno::EINVAL);
            }
            Ok(0)
        }
        PR_SET_PDEATHSIG => {
            if arg2 > 64 {
                return Err(Errno::EINVAL);
            }
            Ok(0)
        }
        PR_GET_PDEATHSIG => {
            if arg2 != 0 {
                let zero = 0i32.to_ne_bytes();
                UserPtr::new(arg2).write_bytes(&vmar, &zero)?;
            }
            Ok(0)
        }
        PR_GET_SECCOMP => {
            let td = Thread::current().ok_or(Errno::ESRCH)?;
            let mode = td.inner.lock().seccomp.mode as usize;
            Ok(mode)
        }
        PR_SET_SECCOMP => match arg2 as u32 {
            SECCOMP_MODE_STRICT => {
                set_mode_strict()?;
                Ok(0)
            }
            SECCOMP_MODE_FILTER => {
                if arg3 == 0 {
                    return Err(Errno::EFAULT);
                }
                let fprog: SockFprog = UserPtr::new(arg3).read_val(&vmar)?;
                if fprog.len == 0 || fprog.len as usize > BPF_MAXINSNS {
                    return Err(Errno::EINVAL);
                }
                if fprog.filter == 0 {
                    return Err(Errno::EFAULT);
                }
                let mut insns = Vec::with_capacity(fprog.len as usize);
                let filter_stride = core::mem::size_of::<SockFilter>();
                for i in 0..fprog.len as usize {
                    let offset = i.checked_mul(filter_stride).ok_or(Errno::EINVAL)?;
                    let addr = fprog.filter.checked_add(offset).ok_or(Errno::EFAULT)?;
                    let insn: SockFilter = UserPtr::new(addr).read_val(&vmar)?;
                    insns.push(insn);
                }
                set_mode_filter(0, insns)?;
                Ok(0)
            }
            _ => Err(Errno::EINVAL),
        },
        PR_SET_CHILD_SUBREAPER => {
            let mut inner = proc.inner.lock();
            inner.is_reaper = arg2 != 0;
            inner.reaper = if arg2 != 0 { Some(proc.pid) } else { None };
            Ok(0)
        }
        PR_GET_CHILD_SUBREAPER => {
            if arg2 == 0 {
                return Err(Errno::EFAULT);
            }
            let is_reaper = proc.inner.lock().is_reaper as i32;
            UserPtr::new(arg2).write_bytes(&vmar, &is_reaper.to_ne_bytes())?;
            Ok(0)
        }
        PR_SET_NO_NEW_PRIVS => {
            if arg2 != 1 || arg3 != 0 || arg4 != 0 || arg5 != 0 {
                return Err(Errno::EINVAL);
            }
            proc.inner.lock().no_new_privs = true;
            Ok(0)
        }
        PR_GET_NO_NEW_PRIVS => {
            if arg2 != 0 || arg3 != 0 || arg4 != 0 || arg5 != 0 {
                return Err(Errno::EINVAL);
            }
            let no_new_privs = proc.inner.lock().no_new_privs as usize;
            Ok(no_new_privs)
        }
        PR_GET_KEEPCAPS => Ok(0),
        PR_SET_KEEPCAPS => Ok(0),
        PR_CAPBSET_DROP => {
            if (proc.inner.lock().cap_effective & (1 << 8)) == 0 {
                return Err(Errno::EPERM);
            }
            Ok(0)
        }
        PR_SET_SECUREBITS => {
            if (proc.inner.lock().cap_effective & (1 << 8)) == 0 {
                return Err(Errno::EPERM);
            }
            proc.inner.lock().securebits = arg2 as u32;
            Ok(0)
        }
        PR_GET_SECUREBITS => Ok(proc.inner.lock().securebits as usize),
        _ => Err(Errno::EINVAL),
    }
}

/// `arch_prctl(2)` - set architecture-specific thread state (Syscall #158 on x86_64).
pub fn sys_arch_prctl(code: i32, addr: usize, _ctx: &mut UserContext) -> Result<usize> {
    let td = Thread::current().ok_or(Errno::ESRCH)?;
    match code {
        ARCH_SET_FS => {
            td.set_fs_base(addr);
            ostd::arch::cpu::context::FsBase::new(addr).load();
            Ok(0)
        }
        ARCH_GET_FS => {
            if addr == 0 {
                return Err(Errno::EFAULT);
            }
            let proc = Thread::proc_or_esrch()?;
            let vmar = proc.vmspace();
            let base = td.fs_base();
            UserPtr::new(addr).write_bytes(&vmar, &base.to_ne_bytes())?;
            Ok(0)
        }
        ARCH_SET_GS => Ok(0),
        ARCH_GET_GS => {
            if addr == 0 {
                return Err(Errno::EFAULT);
            }
            let proc = Thread::proc_or_esrch()?;
            let vmar = proc.vmspace();
            UserPtr::new(addr).write_bytes(&vmar, &0usize.to_ne_bytes())?;
            Ok(0)
        }
        _ => Err(Errno::EINVAL),
    }
}
