use crate::fs::eventfd::{EventFdNode, EventFdOps};
use crate::fs::vfs::FileOps;
use crate::proc::process::Process;
use crate::syscall::{SyscallResult};
use crate::vm::vma::VmaManager;
use alloc::boxed::Box;
use alloc::sync::Arc;
use ostd::arch::cpu::context::UserContext;
use ostd::sync::SpinLock;

/// `eventfd2()` — SYS_eventfd2 = 290
pub fn syscall_eventfd2(
    arg0: usize,
    arg1: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let init_val = arg0 as u64;
    let flags = arg1 as u32;
    let node = Arc::new(EventFdNode {
        counter: SpinLock::new(init_val),
    });
    let ops: Box<dyn FileOps> = Box::new(EventFdOps { node: node.clone() });
    let proc = Process::current();
    SyscallResult::from_result(proc.fd_table.lock().insert_custom(node, ops, flags, 0))
}

/// `eventfd()` — SYS_eventfd = 284
pub fn syscall_eventfd(
    arg0: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    ctx: &mut UserContext,
) -> SyscallResult {
    syscall_eventfd2(arg0, 0, 0, 0, 0, 0, vm, ctx)
}
