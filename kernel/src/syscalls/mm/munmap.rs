//! sys_munmap system call handler.

use crate::syscalls::{SyscallError, SyscallResult};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::mm::VirtAddr;


/// `sys_munmap` (SYS_MUNMAP = 11)
/// Unmap files or devices from memory.
pub fn sys_munmap(frame: &mut SyscallFrame) -> SyscallResult {
    let addr = frame.arg1() as u64;
    let len = frame.arg2() as usize;

    if addr == 0 || !VirtAddr::new(addr).is_aligned(4096u64) || len == 0 {
        return Err(SyscallError::EINVAL);
    }

    let aligned_len = (len + 4095) & !4095;
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();

    let mut addr_space = proc.address_space.lock();
    if addr_space
        .unmap_range(
            VirtAddr::new(addr),
            VirtAddr::new(addr + aligned_len as u64),
        )
        .is_err()
    {
        return Err(SyscallError::EINVAL);
    }

    Ok(0)
}
