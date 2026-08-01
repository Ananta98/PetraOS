use crate::fs::inotify::{InotifyNode, InotifyOps};
use crate::fs::vfs::FileOps;
use crate::proc::process::Process;
use crate::proc::userspace::read_user_string;
use crate::syscall::{SyscallResult};
use crate::vm::vma::VmaManager;
use alloc::boxed::Box;
use alloc::sync::Arc;
use ostd::Error;
use ostd::arch::cpu::context::UserContext;
use ostd::sync::SpinLock;

/// `inotify_init1()` — SYS_inotify_init1 = 294
pub fn syscall_inotify_init1(
    arg0: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let flags = arg0 as u32;
    let node = Arc::new(InotifyNode {
        watch_counter: SpinLock::new(1),
    });
    let ops: Box<dyn FileOps> = Box::new(InotifyOps);
    let proc = Process::current();
    SyscallResult::from_result(proc.fd_table.lock().insert_custom(node, ops, flags, 0))
}

/// `inotify_add_watch()` — SYS_inotify_add_watch = 254
pub fn syscall_inotify_add_watch(
    arg0: usize,
    arg1: usize,
    _arg2: usize,
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let fd = arg0 as i32;
    if read_user_string(vm, arg1).is_err() {
        return SyscallResult::from_err(Error::InvalidArgs);
    }
    let proc = Process::current();
    let fd_table = proc.fd_table.lock();
    if let Ok(fd_entry) = fd_table.get_fd(fd) {
        let open_file = fd_entry.open_file.lock();
        if let Some(ref inode) = open_file.inode {
            if let Some(node) = inode.as_any().downcast_ref::<InotifyNode>() {
                let mut counter = node.watch_counter.lock();
                let wd = *counter;
                *counter += 1;
                return SyscallResult::from_result(Ok(wd));
            }
        }
    }
    SyscallResult::from_result(Ok(1))
}

/// `inotify_rm_watch()` — SYS_inotify_rm_watch = 255
pub fn syscall_inotify_rm_watch(
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    SyscallResult::from_result(Ok(0))
}
