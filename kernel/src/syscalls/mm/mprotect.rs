//! sys_mprotect system call handler.

use crate::mm::{PageTableFlags, VirtAddr};
use crate::syscalls::{wrap_syscall, SyscallError, SyscallResult};

/// `sys_mprotect` (SYS_MPROTECT = 10)
/// Set protection on a region of memory.
#[wrap_syscall]
pub fn sys_mprotect(addr: u64, len: usize, prot: i32) -> SyscallResult {
    if addr == 0 || !VirtAddr::new(addr).is_aligned(4096u64) {
        return Err(SyscallError::EINVAL);
    }

    if len == 0 {
        return Ok(0);
    }

    // Validate protection bits (PROT_NONE=0, PROT_READ=1, PROT_WRITE=2, PROT_EXEC=4)
    if (prot & !0x7) != 0 {
        return Err(SyscallError::EINVAL);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();

    let mut flags = PageTableFlags::USER_ACCESSIBLE;
    if prot != 0 {
        flags |= PageTableFlags::PRESENT;
    }
    if (prot & 2) != 0 {
        flags |= PageTableFlags::WRITABLE;
    }
    if (prot & 4) == 0 {
        flags |= PageTableFlags::NO_EXECUTE;
    }

    let mut addr_space = proc.address_space.lock();
    match addr_space.mprotect_range(VirtAddr::new(addr), len, flags) {
        Ok(()) => Ok(0),
        Err(crate::mm::AddrSpaceError::UnmappedRange) => Err(SyscallError::ENOMEM),
        Err(crate::mm::AddrSpaceError::InvalidRange) => Err(SyscallError::EINVAL),
        Err(crate::mm::AddrSpaceError::PagingError(_))
        | Err(crate::mm::AddrSpaceError::FlagUpdateError(_)) => Err(SyscallError::ENOMEM),
        Err(_) => Err(SyscallError::EINVAL),
    }
}
