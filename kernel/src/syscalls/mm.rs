use super::{SyscallError, SyscallResult};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::mm::vmm::vma::VmAreaKind;
use crate::mm::{MapFlags, VirtAddr};
use alloc::sync::Arc;

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
            let flags = MapFlags::READ | MapFlags::WRITE | MapFlags::USER;

            // SAFETY: `proc` lock is held exclusively by active process thread during syscall execution.
            let addr_space = unsafe {
                &mut *(Arc::as_ptr(&proc.address_space)
                    as *mut crate::mm::vmm::AddrSpace<crate::arch::paging::ArchPageTable>)
            };
            let _ = addr_space.map_area(
                VirtAddr(page_start),
                size,
                flags,
                VmAreaKind::Anonymous,
            );
        }
    }

    proc.heap_brk = new_brk;
    Ok(new_brk as usize)
}

/// `sys_mmap` (SYS_MMAP = 9)
/// Map files or devices into memory.
pub fn sys_mmap(frame: &mut SyscallFrame) -> SyscallResult {
    let addr = frame.arg1() as u64;
    let len = frame.arg2() as usize;
    let _prot = frame.arg3() as i32;
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
        addr & !4095
    } else {
        let vaddr = proc.mmap_bump;
        proc.mmap_bump += aligned_len as u64;
        vaddr
    };

    let map_flags = MapFlags::READ | MapFlags::WRITE | MapFlags::USER;

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

    // SAFETY: `proc` lock is held exclusively by active process thread during syscall execution.
    let addr_space = unsafe {
        &mut *(Arc::as_ptr(&proc.address_space)
            as *mut crate::mm::vmm::AddrSpace<crate::arch::paging::ArchPageTable>)
    };

    if addr_space
        .map_area(
            VirtAddr(target_vaddr),
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

/// `sys_munmap` (SYS_MUNMAP = 11)
/// Unmap files or devices from memory.
pub fn sys_munmap(frame: &mut SyscallFrame) -> SyscallResult {
    let addr = frame.arg1() as u64;
    let len = frame.arg2() as usize;

    if addr == 0 || !VirtAddr(addr).is_aligned(4096) || len == 0 {
        return Err(SyscallError::EINVAL);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();

    // SAFETY: `proc` lock is held exclusively by active process thread during syscall execution.
    let addr_space = unsafe {
        &mut *(Arc::as_ptr(&proc.address_space)
            as *mut crate::mm::vmm::AddrSpace<crate::arch::paging::ArchPageTable>)
    };

    if addr_space.unmap_area(VirtAddr(addr)).is_err() {
        return Err(SyscallError::EINVAL);
    }

    Ok(0)
}

/// `sys_mprotect` (SYS_MPROTECT = 10)
/// Set protection on a region of memory.
pub fn sys_mprotect(frame: &mut SyscallFrame) -> SyscallResult {
    let addr = frame.arg1() as u64;
    let len = frame.arg2() as usize;

    if addr == 0 || len == 0 {
        return Err(SyscallError::EINVAL);
    }

    Ok(0)
}
