use crate::fs::signalfd::{SignalFdNode, SignalFdOps};
use crate::fs::vfs::FileOps;
use crate::proc::process::Process;
use crate::syscall::{SyscallResult};
use crate::vm::vma::VmaManager;
use alloc::boxed::Box;
use alloc::sync::Arc;
use ostd::arch::cpu::context::UserContext;

/// `signalfd4()` — SYS_signalfd4 = 289
pub fn syscall_signalfd4(
    _fd: usize,
    _mask_ptr: usize,
    _mask_len: usize,
    arg3: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let flags = arg3 as u32;
    let node = Arc::new(SignalFdNode);
    let ops: Box<dyn FileOps> = Box::new(SignalFdOps);
    let proc = Process::current();
    SyscallResult::from_result(proc.fd_table.lock().insert_custom(node, ops, flags, 0))
}
