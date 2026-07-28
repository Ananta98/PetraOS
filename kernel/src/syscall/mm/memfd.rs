use crate::fs::ramfs::RamfsInode;
use crate::fs::vfs::{FileOps, FileType, InodeOps};
use crate::proc::process::Process;
use crate::proc::userspace::read_user_string;
use crate::syscall::{SyscallResult, to_continue_i32};
use crate::vm::vma::VmaManager;
use alloc::boxed::Box;
use alloc::sync::Arc;
use ostd::Error;
use ostd::arch::cpu::context::UserContext;

/// `memfd_create()` — SYS_memfd_create = 319
pub fn syscall_memfd_create(
    arg0: usize,
    arg1: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let _name = match read_user_string(vm, arg0) {
        Ok(n) => n,
        Err(e) => return to_continue_i32(Err(e)),
    };
    let flags = arg1 as u32;

    let inode = RamfsInode::new(FileType::Regular, 0o600);
    let ops: Box<dyn FileOps> = match inode.open(0) {
        Ok(o) => o,
        Err(e) => return to_continue_i32(Err(e)),
    };

    let proc = Process::current();
    to_continue_i32(proc.fd_table.lock().insert_custom(inode, ops, flags, 0))
}
