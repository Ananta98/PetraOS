//! sys_brk system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::mm::{PageTable, PageTableFlags, VirtAddr, VmAreaKind};


/// `sys_brk` (SYS_BRK = 12)
/// Change data segment size (heap break pointer).
pub fn sys_brk(frame: &mut SyscallFrame) -> SyscallResult {
    let new_brk = frame.arg1() as u64;

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let mut proc = proc_arc.lock();

    let current_brk = proc.heap_brk;
    if new_brk == 0 || new_brk < proc.heap_start {
        return Ok(current_brk as usize);
    }

    if new_brk > current_brk {
        // Expand heap range
        let page_start = (current_brk + 4095) & !4095;
        let page_end = (new_brk + 4095) & !4095;

        if page_end > page_start {
            let size = (page_end - page_start) as usize;
            let flags = PageTableFlags::PRESENT
                | PageTableFlags::WRITABLE
                | PageTableFlags::USER_ACCESSIBLE;

            let mut addr_space = proc.address_space.lock();
            let _ = addr_space.map_area(
                VirtAddr::new(page_start),
                size,
                flags,
                VmAreaKind::Anonymous,
            );
        }
    }

    proc.heap_brk = new_brk;
    Ok(new_brk as usize)
}
