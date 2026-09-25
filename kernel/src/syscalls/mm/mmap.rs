//! sys_mmap system call handler.

use crate::mm::{PageTableFlags, VirtAddr, VmAreaKind};
use crate::syscalls::{wrap_syscall, SyscallError, SyscallResult};

/// `sys_mmap` (SYS_MMAP = 9)
/// Map files or devices into memory.
#[wrap_syscall]
pub fn sys_mmap(
    addr: u64,
    len: usize,
    prot: i32,
    flags: i32,
    fd: i32,
    offset: u64,
) -> SyscallResult {
    if len == 0 {
        return Err(SyscallError::EINVAL);
    }

    // Linux mmap flag bits (x86_64).
    const MAP_SHARED: i32 = 0x01;
    const MAP_PRIVATE: i32 = 0x02;
    const MAP_FIXED: i32 = 0x10;
    const MAP_ANONYMOUS: i32 = 0x20;
    // MAP_FIXED_NOREPLACE (0x100000) is intentionally not supported yet:
    // treat as MAP_FIXED for replacement semantics.
    let _ = MAP_SHARED;
    let _ = MAP_PRIVATE;

    let is_fixed = (flags & MAP_FIXED) != 0;
    let is_anonymous = (flags & MAP_ANONYMOUS) != 0;

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();

    let aligned_len = (len + 4095) & !4095;
    let target_vaddr = if is_fixed {
        // MAP_FIXED: map at exactly the requested address, replacing any
        // existing overlapping range.
        if addr == 0 {
            return Err(SyscallError::EINVAL);
        }
        let vaddr = addr & !4095;
        let mut addr_space = proc.address_space.lock();
        let _ = addr_space.unmap_range(
            VirtAddr::new(vaddr),
            VirtAddr::new(vaddr + aligned_len as u64),
        );
        drop(addr_space);
        vaddr
    } else if addr != 0 {
        // Hint: use it only if the whole range is free, otherwise fall back
        // to a free range. Never destroy existing mappings for a hint.
        let vaddr = addr & !4095;
        let end = vaddr + aligned_len as u64;
        let use_hint = {
            let addr_space = proc.address_space.lock();
            !addr_space.check_overlap(VirtAddr::new(vaddr), VirtAddr::new(end))
        };
        if use_hint {
            vaddr
        } else {
            let addr_space = proc.address_space.lock();
            let free_vaddr = addr_space
                .find_free_range(aligned_len, 4096)
                .ok_or(SyscallError::ENOMEM)?;
            drop(addr_space);
            free_vaddr.as_u64()
        }
    } else {
        let addr_space = proc.address_space.lock();
        let free_vaddr = addr_space
            .find_free_range(aligned_len, 4096)
            .ok_or(SyscallError::ENOMEM)?;
        drop(addr_space);
        free_vaddr.as_u64()
    };

    let mut map_flags = PageTableFlags::USER_ACCESSIBLE;
    if prot != 0 {
        map_flags |= PageTableFlags::PRESENT;
    }
    if (prot & 2) != 0 {
        map_flags |= PageTableFlags::WRITABLE;
    }
    if (prot & 4) == 0 {
        map_flags |= PageTableFlags::NO_EXECUTE;
    }

    let kind = if !is_anonymous && fd >= 0 {
        if let Ok(file) = proc.fd_table.get(fd) {
            if file.dentry.name.starts_with("fb") {
                if let Some(fb_info) = crate::drivers::drm::get_framebuffer_info() {
                    let hhdm = crate::mm::hhdm_offset();
                    let phys_addr = fb_info.address.saturating_sub(hhdm).saturating_add(offset);
                    VmAreaKind::Device {
                        phys_start: crate::mm::PhysAddr::new(phys_addr),
                    }
                } else {
                    let file_size = file.ops.stat().map(|s| s.size as usize).unwrap_or(0);
                    VmAreaKind::File {
                        file: file.ops.clone(),
                        offset: offset as usize,
                        file_size,
                    }
                }
            } else {
                let file_size = file.ops.stat().map(|s| s.size as usize).unwrap_or(0);
                VmAreaKind::File {
                    file: file.ops.clone(),
                    offset: offset as usize,
                    file_size,
                }
            }
        } else {
            VmAreaKind::Anonymous
        }
    } else {
        VmAreaKind::Anonymous
    };

    let mut addr_space = proc.address_space.lock();
    if addr_space
        .map_area(VirtAddr::new(target_vaddr), aligned_len, map_flags, kind)
        .is_err()
    {
        return Err(SyscallError::ENOMEM);
    }

    Ok(target_vaddr as usize)
}
