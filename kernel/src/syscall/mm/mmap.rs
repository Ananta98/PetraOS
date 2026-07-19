use crate::fs::fd_table::OpenFile;
use crate::fs::vfs::FileOps;
use crate::proc::process::Process;
use crate::syscall::SyscallResult;
use crate::vm::vma::VmaManager;
use alloc::sync::Arc;
use ostd::Error;
use ostd::mm::{PAGE_SIZE, PageFlags};
use ostd::sync::SpinLock;

// ---------------------------------------------------------------------------
// Syscall implementation
// ---------------------------------------------------------------------------

const PROT_READ: usize = 0x1;
const PROT_WRITE: usize = 0x2;
const PROT_EXEC: usize = 0x4;
const PROT_NONE: usize = 0x0;

const MAP_SHARED: usize = 0x01;
const MAP_ANONYMOUS: usize = 0x20;
const MAP_FIXED: usize = 0x10;
const MAP_POPULATE: usize = 0x08000;

fn prot_to_pageflags(prot: usize) -> PageFlags {
    if prot == PROT_NONE || prot == 0 {
        return PageFlags::empty();
    }
    let mut flags = PageFlags::empty();
    if prot & PROT_READ != 0 {
        flags |= PageFlags::R;
    }
    if prot & PROT_WRITE != 0 {
        flags |= PageFlags::W;
    }
    if prot & PROT_EXEC != 0 {
        flags |= PageFlags::X;
    }
    flags
}

pub fn syscall_mmap(
    arg0: usize,
    arg1: usize,
    arg2: usize,
    arg3: usize,
    arg4: usize,
    arg5: usize,
    vm: &VmaManager,
    _: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    let addr = arg0;
    let length = arg1;
    let prot = arg2;
    let flags = arg3;
    let fd = arg4 as i32;
    let offset = arg5;

    let page_flags = prot_to_pageflags(prot);
    let is_anonymous = (flags & MAP_ANONYMOUS) != 0;
    let is_shared = (flags & MAP_SHARED) != 0;
    let is_fixed = (flags & MAP_FIXED) != 0;
    let populate = (flags & MAP_POPULATE) != 0;

    if length == 0 {
        return SyscallResult::Continue(-(Error::InvalidArgs as isize) as usize);
    }

    let addr_opt = if addr != 0 {
        if is_fixed {
            Some(addr)
        } else {
            // addr is just a hint, kernel may ignore it
            None
        }
    } else {
        None
    };

    let result = if is_anonymous {
        // Anonymous mapping
        vm.mmap_anon(addr_opt, length, page_flags, populate)
    } else {
        // File-backed mapping
        let proc = Process::current();
        let fd_table = proc.fd_table.lock();

        let fd_entry = match fd_table.get_fd(fd) {
            Ok(entry) => entry,
            Err(e) => {
                return SyscallResult::Continue(-(e as isize) as usize);
            }
        };

        if offset % PAGE_SIZE != 0 {
            return SyscallResult::Continue(-(Error::InvalidArgs as isize) as usize);
        }

        let backing = fd_entry.open_file.clone();

        vm.mmap_file(addr_opt, length, page_flags, backing, offset, is_shared)
    };

    match result {
        Ok(vaddr) => SyscallResult::Continue(vaddr),
        Err(e) => SyscallResult::Continue(-(e as isize) as usize),
    }
}
