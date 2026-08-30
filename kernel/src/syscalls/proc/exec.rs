//! Program execution system call (`execve`).

use super::*;
use crate::arch::syscall::syscall::SyscallFrame;
use crate::mm::vmm::paging::PageTable;
use crate::syscalls::{SyscallError, SyscallResult, UserCStr};

/// `sys_execve` (SYS_EXECVE = 59)
/// Execute program file.
pub fn sys_execve(frame: &mut SyscallFrame) -> SyscallResult {
    let path_ptr = UserCStr::from_u64(frame.arg1());
    let argv_ptr = frame.arg2() as *const *const u8;
    let envp_ptr = frame.arg3() as *const *const u8;

    let path = path_ptr.to_string(256)?;

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let mut proc = proc_arc.lock();

    let (entry_point, stack_top) = proc
        .execute(&path, 0, argv_ptr, envp_ptr)
        .map_err(|_| SyscallError::ENOENT)?;

    let new_cr3 = proc.address_space.lock().page_table().root().as_u64();

    // SAFETY: Switching CPU page directory to the newly executed program's address space.
    unsafe {
        crate::arch::set_address_space_root(new_cr3);
    }

    if let Some(thread_arc) = crate::proc::current_thread() {
        let mut t = thread_arc.lock();
        t.context.cr3 = new_cr3 as usize;
        t.context.fs_base = 0;
    }
    crate::arch::cpu::msr::write_fs_base(0);

    frame.rip = entry_point;
    frame.rsp = stack_top;

    Ok(0)
}
