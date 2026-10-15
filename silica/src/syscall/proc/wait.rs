// SPDX-License-Identifier: GPL-2.0

use crate::{
    api::errno::{Errno, Result},
    proc::{
        thread::Thread,
        wait::{IdType, WaitOptions, kernel_wait},
    },
};
use ostd::arch::cpu::context::UserContext;

/// sys_wait4(pid, status, options, rusage)
pub fn sys_wait4(
    pid_val: i32,
    status_ptr: usize,
    options_val: u32,
    _rusage_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;

    let idtype = if pid_val < -1 {
        IdType::Pgid((-pid_val) as u32)
    } else if pid_val == -1 {
        IdType::All
    } else if pid_val == 0 {
        IdType::Pgid(proc.pgid().as_u32()) // wait for any child in same pgid
    } else {
        let pid = crate::proc::tree::Pid::from_u32(pid_val as u32).ok_or(Errno::EINVAL)?;
        IdType::Pid(pid)
    };

    let options = WaitOptions::from_bits_truncate(options_val);
    let res = kernel_wait(&proc, idtype, options)?;

    match res {
        Some(wait_res) => {
            if status_ptr != 0 {
                let vmar = proc.vmspace();
                let raw_status = wait_res.status.as_raw();
                crate::utils::userspace::UserPtr::new(status_ptr).write_bytes(&vmar, &raw_status.to_ne_bytes())?;
            }
            Ok(wait_res.pid.as_u32() as usize)
        }
        None => Ok(0),
    }
}
