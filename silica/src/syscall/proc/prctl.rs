// SPDX-License-Identifier: GPL-2.0

//! `prctl(2)` system call (Syscall #157 on x86_64).

use ostd::arch::cpu::context::UserContext;

use crate::{
    api::errno::{Errno, Result},
    proc::thread::Thread,
    utils::userspace::user_ptr::UserPtr,
};

const PR_SET_PDEATHSIG: i32 = 1;
const PR_GET_PDEATHSIG: i32 = 2;
const PR_GET_DUMPABLE: i32 = 3;
const PR_SET_DUMPABLE: i32 = 4;
const PR_SET_NAME: i32 = 15;
const PR_GET_NAME: i32 = 16;
const PR_SET_CHILD_SUBREAPER: i32 = 36;
const PR_GET_CHILD_SUBREAPER: i32 = 37;

const COMM_LEN: usize = 16;

pub const ARCH_SET_GS: i32 = 0x1001;
pub const ARCH_SET_FS: i32 = 0x1002;
pub const ARCH_GET_FS: i32 = 0x1003;
pub const ARCH_GET_GS: i32 = 0x1004;

/// `prctl(2)` - operations on a process or thread.
pub fn sys_prctl(
    option: i32,
    arg2: usize,
    _arg3: usize,
    _arg4: usize,
    _arg5: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
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
        PR_SET_DUMPABLE => Ok(0),
        PR_SET_PDEATHSIG => Ok(0),
        PR_GET_PDEATHSIG => {
            if arg2 != 0 {
                let zero = 0i32.to_ne_bytes();
                UserPtr::new(arg2).write_bytes(&vmar, &zero)?;
            }
            Ok(0)
        }
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
            let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
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
            let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
            let vmar = proc.vmspace();
            UserPtr::new(addr).write_bytes(&vmar, &0usize.to_ne_bytes())?;
            Ok(0)
        }
        _ => Err(Errno::EINVAL),
    }
}
