use crate::fs::timerfd::{TimerFdNode, TimerFdOps};
use crate::fs::vfs::FileOps;
use crate::proc::process::Process;
use crate::syscall::{SyscallResult, to_continue_i32};
use crate::vm::vma::VmaManager;
use alloc::boxed::Box;
use alloc::sync::Arc;
use ostd::Error;
use ostd::arch::cpu::context::UserContext;
use ostd::sync::SpinLock;

/// `timerfd_create()` — SYS_timerfd_create = 283
pub fn syscall_timerfd_create(
    _clockid: usize,
    arg1: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let flags = arg1 as u32;
    let node = Arc::new(TimerFdNode {
        timer_ticks: SpinLock::new(1),
    });
    let ops: Box<dyn FileOps> = Box::new(TimerFdOps { node: node.clone() });
    let proc = Process::current();
    to_continue_i32(proc.fd_table.lock().insert_custom(node, ops, flags, 0))
}

/// `timerfd_settime()` — SYS_timerfd_settime = 286
pub fn syscall_timerfd_settime(
    arg0: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let fd = arg0 as i32;
    let proc = Process::current();
    let fd_table = proc.fd_table.lock();
    if fd_table.get_fd(fd).is_err() {
        return to_continue_i32(Err(Error::InvalidArgs));
    }
    to_continue_i32(Ok(0))
}

/// `timerfd_gettime()` — SYS_timerfd_gettime = 287
pub fn syscall_timerfd_gettime(
    arg0: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let fd = arg0 as i32;
    let proc = Process::current();
    let fd_table = proc.fd_table.lock();
    if fd_table.get_fd(fd).is_err() {
        return to_continue_i32(Err(Error::InvalidArgs));
    }
    to_continue_i32(Ok(0))
}
