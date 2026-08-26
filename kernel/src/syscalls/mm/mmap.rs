//! sys_mmap system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::mm::{PageTable, PageTableFlags, VirtAddr, VmAreaKind};


/// `sys_mmap` (SYS_MMAP = 9)
/// Map files or devices into memory.
pub fn sys_mmap(frame: &mut SyscallFrame) -> SyscallResult {
    let addr = frame.arg1() as u64;
    let len = frame.arg2() as usize;
    let prot = frame.arg3() as i32;
    let _flags = frame.arg4() as i32;
    let fd = frame.arg5() as i32;
    let offset = frame.arg6() as u64;

    if len == 0 {
        return Err(SyscallError::EINVAL);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let mut proc = proc_arc.lock();

    let aligned_len = (len + 4095) & !4095;
    let target_vaddr = if addr != 0 {
        let vaddr = addr & !4095;
        // POSIX MAP_FIXED replacement: unmap any existing overlapping range
        let mut addr_space = proc.address_space.lock();
        let _ = addr_space.unmap_range(
            VirtAddr::new(vaddr),
            VirtAddr::new(vaddr + aligned_len as u64),
        );
        drop(addr_space);
        vaddr
    } else {
        let vaddr = proc.mmap_bump;
        proc.mmap_bump += aligned_len as u64;
        vaddr
    };

    let mut map_flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
    if (prot & 2) != 0 {
        map_flags |= PageTableFlags::WRITABLE;
    }
    if (prot & 4) == 0 {
        map_flags |= PageTableFlags::NO_EXECUTE;
    }

    let kind = if fd >= 0 {
        if let Ok(file) = proc.fd_table.get(fd) {
            let file_size = file.ops.stat().map(|s| s.size as usize).unwrap_or(0);
            VmAreaKind::File {
                file: file.ops.clone(),
                offset: offset as usize,
                file_size,
            }
        } else {
            VmAreaKind::Anonymous
        }
    } else {
        VmAreaKind::Anonymous
    };

    let mut addr_space = proc.address_space.lock();
    if addr_space
        .map_area(
            VirtAddr::new(target_vaddr),
            aligned_len,
            map_flags,
            kind,
        )
        .is_err()
    {
        return Err(SyscallError::ENOMEM);
    }

    Ok(target_vaddr as usize)
}
