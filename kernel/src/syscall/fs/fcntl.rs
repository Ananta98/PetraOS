use crate::proc::process::Process;
use crate::syscall::{SyscallResult, to_continue_i32};
use crate::vm::vma::VmaManager;
use ostd::Error;
use ostd::arch::cpu::context::UserContext;

pub const F_DUPFD: usize = 0;
pub const F_GETFD: usize = 1;
pub const F_SETFD: usize = 2;
pub const F_GETFL: usize = 3;
pub const F_SETFL: usize = 4;
pub const F_DUPFD_CLOEXEC: usize = 1030;

/// `fcntl()` — SYS_fcntl = 72
pub fn syscall_fcntl(
    arg0: usize,
    arg1: usize,
    arg2: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let fd = arg0 as i32;
    let cmd = arg1;
    let proc = Process::current();
    let mut fd_table = proc.fd_table.lock();

    match cmd {
        F_DUPFD | F_DUPFD_CLOEXEC => {
            let min_fd = arg2 as i32;
            let file = match fd_table.get_fd(fd) {
                Ok(f) => f,
                Err(e) => return to_continue_i32(Err(e)),
            };
            let cloexec = cmd == F_DUPFD_CLOEXEC;
            let new_fd = match fd_table.insert_at_or_above(file, min_fd, cloexec) {
                Ok(n) => n,
                Err(e) => return to_continue_i32(Err(e)),
            };
            to_continue_i32(Ok(new_fd))
        }
        F_GETFD => {
            if fd_table.get_fd(fd).is_err() {
                to_continue_i32(Err(Error::InvalidArgs))
            } else {
                let flags = if fd_table.is_cloexec(fd) { 1 } else { 0 };
                to_continue_i32(Ok(flags))
            }
        }
        F_SETFD => {
            let cloexec = (arg2 & 1) != 0;
            if fd_table.set_cloexec(fd, cloexec).is_ok() {
                to_continue_i32(Ok(0))
            } else {
                to_continue_i32(Err(Error::InvalidArgs))
            }
        }
        F_GETFL => {
            if let Ok(fd_entry) = fd_table.get_fd(fd) {
                let flags = fd_entry.open_file.lock().flags;
                to_continue_i32(Ok(flags as i32))
            } else {
                to_continue_i32(Err(Error::InvalidArgs))
            }
        }
        F_SETFL => {
            if let Ok(fd_entry) = fd_table.get_fd(fd) {
                fd_entry.open_file.lock().flags = arg2 as u32;
                to_continue_i32(Ok(0))
            } else {
                to_continue_i32(Err(Error::InvalidArgs))
            }
        }
        _ => to_continue_i32(Ok(0)),
    }
}
