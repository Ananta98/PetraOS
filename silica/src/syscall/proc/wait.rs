// SPDX-License-Identifier: GPL-2.0

//! Process wait system calls (`wait4` and `waitid`).

use ostd::arch::cpu::context::UserContext;

use crate::{
    proc::{
        ExitStatus, Pid,
        lifecycle::{IdType, WaitOptions, kernel_wait},
        thread::Thread,
    },
    uapi::{
        errno::{Errno, Result},
        signal::Signal,
    },
    utils::userspace::UserPtr,
};

pub const P_ALL: i32 = 0;
pub const P_PID: i32 = 1;
pub const P_PGID: i32 = 2;

// waitid options
pub const WNOHANG: i32 = 1;
pub const WSTOPPED: i32 = 2;
pub const WEXITED: i32 = 4;
pub const WCONTINUED: i32 = 8;
pub const WNOWAIT: i32 = 0x0100_0000;

// POSIX siginfo codes for waitid
pub const CLD_EXITED: i32 = 1;
pub const CLD_KILLED: i32 = 2;
pub const CLD_DUMPED: i32 = 3;
pub const CLD_TRAPPED: i32 = 4;
pub const CLD_STOPPED: i32 = 5;
pub const CLD_CONTINUED: i32 = 6;

/// `wait4(pid, status, options, rusage)` (Syscall #61 on x86_64).
pub fn sys_wait4(
    pid_val: i32,
    status_ptr: usize,
    options_val: u32,
    rusage_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;

    let idtype = if pid_val < -1 {
        IdType::Pgid((-pid_val) as u32)
    } else if pid_val == -1 {
        IdType::All
    } else if pid_val == 0 {
        IdType::Pgid(proc.pgid().as_u32())
    } else {
        let pid = Pid::from_u32(pid_val as u32).ok_or(Errno::EINVAL)?;
        IdType::Pid(pid)
    };

    let options = WaitOptions::from_bits_truncate(options_val);
    let res = kernel_wait(&proc, idtype, options)?;

    match res {
        Some(wait_res) => {
            let vmar = proc.vmspace();
            if status_ptr != 0 {
                let raw_status = wait_res.status.as_raw();
                UserPtr::new(status_ptr).write_bytes(&vmar, &raw_status.to_ne_bytes())?;
            }
            if rusage_ptr != 0 {
                let rusage = proc.inner.lock().children_rusage;
                UserPtr::new(rusage_ptr).write_bytes(&vmar, &rusage.to_bytes())?;
            }
            Ok(wait_res.pid.as_u32() as usize)
        }
        None => Ok(0),
    }
}

/// `waitid(which, upid, infop, options, ru)` (Syscall #247 on x86_64).
pub fn sys_waitid(
    which: i32,
    upid: i32,
    infop: usize,
    options_val: i32,
    rusage_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;

    let idtype = match which {
        P_ALL => IdType::All,
        P_PID => {
            if upid <= 0 {
                return Err(Errno::EINVAL);
            }
            let pid = Pid::from_u32(upid as u32).ok_or(Errno::EINVAL)?;
            IdType::Pid(pid)
        }
        P_PGID => {
            if upid < 0 {
                return Err(Errno::EINVAL);
            }
            let pgid = if upid == 0 {
                proc.pgid().as_u32()
            } else {
                upid as u32
            };
            IdType::Pgid(pgid)
        }
        _ => return Err(Errno::EINVAL),
    };

    let mut opts = WaitOptions::empty();
    if (options_val & WNOHANG) != 0 {
        opts |= WaitOptions::WNOHANG;
    }
    if (options_val & WEXITED) != 0 {
        opts |= WaitOptions::WEXITED;
    }
    if (options_val & WSTOPPED) != 0 {
        opts |= WaitOptions::WSTOPPED;
    }
    if (options_val & WCONTINUED) != 0 {
        opts |= WaitOptions::WCONTINUED;
    }
    if (options_val & WNOWAIT) != 0 {
        opts |= WaitOptions::WNOWAIT;
    }

    // POSIX requirement: at least one of WEXITED, WSTOPPED, WCONTINUED must be specified
    if !opts.intersects(WaitOptions::WEXITED | WaitOptions::WSTOPPED | WaitOptions::WCONTINUED) {
        return Err(Errno::EINVAL);
    }

    let res = kernel_wait(&proc, idtype, opts)?;
    let vmar = proc.vmspace();

    match res {
        Some(wait_res) => {
            if infop != 0 {
                let mut siginfo_bytes = [0u8; 128];
                // si_signo = SIGCHLD (17)
                siginfo_bytes[0..4]
                    .copy_from_slice(&(Signal::SIGCHLD.as_u32() as i32).to_ne_bytes());
                // si_errno = 0
                siginfo_bytes[4..8].copy_from_slice(&0i32.to_ne_bytes());

                let (si_code, si_status) = match wait_res.status {
                    ExitStatus::Exited(code) => (CLD_EXITED, code),
                    ExitStatus::Signaled {
                        signal,
                        core_dumped,
                    } => {
                        let code = if core_dumped { CLD_DUMPED } else { CLD_KILLED };
                        (code, signal.as_u32() as i32)
                    }
                    ExitStatus::Stopped(signal) => (CLD_STOPPED, signal.as_u32() as i32),
                    ExitStatus::Continued => (CLD_CONTINUED, Signal::SIGCONT.as_u32() as i32),
                };

                // si_code at offset 8
                siginfo_bytes[8..12].copy_from_slice(&si_code.to_ne_bytes());
                // si_pid at offset 16 (x86_64 siginfo layout)
                siginfo_bytes[16..20]
                    .copy_from_slice(&(wait_res.pid.as_u32() as i32).to_ne_bytes());
                // si_uid at offset 20
                siginfo_bytes[20..24].copy_from_slice(&(wait_res.uid as i32).to_ne_bytes());
                // si_status at offset 24
                siginfo_bytes[24..28].copy_from_slice(&si_status.to_ne_bytes());

                UserPtr::new(infop).write_bytes(&vmar, &siginfo_bytes)?;
            }

            if rusage_ptr != 0 {
                let rusage = proc.inner.lock().children_rusage;
                UserPtr::new(rusage_ptr).write_bytes(&vmar, &rusage.to_bytes())?;
            }

            Ok(0)
        }
        None => {
            // WNOHANG and no child was ready: clear si_pid to 0 as required by POSIX
            if infop != 0 {
                let siginfo_bytes = [0u8; 128];
                UserPtr::new(infop).write_bytes(&vmar, &siginfo_bytes)?;
            }
            Ok(0)
        }
    }
}
