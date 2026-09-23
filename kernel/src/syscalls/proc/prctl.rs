//! Process control system call (`prctl`).
//!
//! Handles:
//! - `sys_prctl` (SYS_PRCTL = 157)

use crate::arch::syscall::syscall::SyscallFrame;
use crate::syscalls::{SyscallError, SyscallResult, UserPtr};

pub const PR_SET_PDEATHSIG: i32 = 1;
pub const PR_GET_PDEATHSIG: i32 = 2;
pub const PR_GET_DUMPABLE: i32 = 3;
pub const PR_SET_DUMPABLE: i32 = 4;
pub const PR_SET_NAME: i32 = 15;
pub const PR_GET_NAME: i32 = 16;

/// `sys_prctl` (SYS_PRCTL = 157)
/// Operations on a process or thread.
pub fn sys_prctl(frame: &mut SyscallFrame) -> SyscallResult {
    let option = frame.arg1() as i32;
    let arg2 = frame.arg2();

    match option {
        PR_SET_NAME => {
            let name_ptr = UserPtr::<u8>::from_u64(arg2);
            if name_ptr.is_null() {
                return Err(SyscallError::EFAULT);
            }
            let mut name_buf = [0u8; 16];
            let _ = name_ptr.read_slice(&mut name_buf);
            Ok(0)
        }
        PR_GET_NAME => {
            let name_ptr = UserPtr::<u8>::from_u64(arg2);
            if name_ptr.is_null() {
                return Err(SyscallError::EFAULT);
            }
            let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
            let proc = proc_arc.lock();
            let mut name_buf = [0u8; 16];
            let name_bytes = proc.cmdline.args.first().map(|s| s.as_bytes()).unwrap_or(b"");
            let len = core::cmp::min(name_bytes.len(), 15);
            name_buf[..len].copy_from_slice(&name_bytes[..len]);
            name_buf[len] = 0;
            drop(proc);

            name_ptr
                .write_slice(&name_buf)
                .ok_or(SyscallError::EFAULT)?;
            Ok(0)
        }
        PR_SET_PDEATHSIG => {
            // Signal to send when parent process dies.
            let sig = arg2 as i32;
            if sig < 0 || sig > 64 {
                return Err(SyscallError::EINVAL);
            }
            Ok(0)
        }
        PR_GET_PDEATHSIG => {
            let out_ptr = UserPtr::<i32>::from_u64(arg2);
            if out_ptr.is_null() {
                return Err(SyscallError::EFAULT);
            }
            out_ptr.write(0).ok_or(SyscallError::EFAULT)?;
            Ok(0)
        }
        PR_SET_DUMPABLE => Ok(0),
        PR_GET_DUMPABLE => Ok(1),
        _ => {
            log::debug!("[prctl] Unsupported prctl option: {}", option);
            Err(SyscallError::EINVAL)
        }
    }
}
