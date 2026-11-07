// SPDX-License-Identifier: GPL-2.0

//! System call `seccomp(2)` (Syscall #317 on x86_64).
//!
//! Provides strict and classic BPF-filtered execution sandboxing according
//! to Linux ABI specifications.

use alloc::vec::Vec;
use ostd::arch::cpu::context::UserContext;

use crate::{
    proc::{
        seccomp::{BPF_MAXINSNS, get_action_avail, set_mode_filter, set_mode_strict},
        thread::Thread,
    },
    uapi::{
        errno::{Errno, Result},
        seccomp::*,
    },
    utils::userspace::UserPtr,
};

/// System call `seccomp(unsigned int operation, unsigned int flags, void *args)` (Syscall #317).
pub fn sys_seccomp(
    operation: u32,
    flags: u32,
    args_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    match operation {
        SECCOMP_SET_MODE_STRICT => {
            if flags != 0 || args_ptr != 0 {
                return Err(Errno::EINVAL);
            }
            set_mode_strict()?;
            Ok(0)
        }
        SECCOMP_SET_MODE_FILTER => {
            if flags & !SECCOMP_FILTER_FLAG_MASK != 0 {
                return Err(Errno::EINVAL);
            }
            if args_ptr == 0 {
                return Err(Errno::EFAULT);
            }

            let proc = Thread::proc_or_esrch()?;
            let vmar = proc.vmspace();
            let fprog: SockFprog = UserPtr::new(args_ptr).read_val(&vmar)?;

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

            set_mode_filter(flags, insns)?;
            Ok(0)
        }
        SECCOMP_GET_ACTION_AVAIL => {
            if flags != 0 {
                return Err(Errno::EINVAL);
            }
            if args_ptr == 0 {
                return Err(Errno::EFAULT);
            }

            let proc = Thread::proc_or_esrch()?;
            let vmar = proc.vmspace();
            let action: u32 = UserPtr::new(args_ptr).read_val(&vmar)?;
            get_action_avail(action)?;
            Ok(0)
        }
        SECCOMP_GET_NOTIF_SIZES => {
            if flags != 0 {
                return Err(Errno::EINVAL);
            }
            if args_ptr == 0 {
                return Err(Errno::EFAULT);
            }

            let proc = Thread::proc_or_esrch()?;
            let vmar = proc.vmspace();
            let sizes = SeccompNotifSizes::standard();
            UserPtr::new(args_ptr).write_val(&vmar, sizes)?;
            Ok(0)
        }
        _ => Err(Errno::EINVAL),
    }
}
