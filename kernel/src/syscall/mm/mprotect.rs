use crate::syscall::SyscallResult;
use crate::vm::vma::VmaManager;
use ostd::arch::cpu::context::UserContext;
use ostd::mm::PageFlags;

const PROT_READ: usize = 0x1;
const PROT_WRITE: usize = 0x2;
const PROT_EXEC: usize = 0x4;
const PROT_NONE: usize = 0x0;

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

pub fn syscall_mprotect(
    arg0: usize,
    arg1: usize,
    arg2: usize,
    _arg3: usize,
    _arg4: usize,
    _arg5: usize,
    vm: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let addr = arg0;
    let len = arg1;
    let prot = arg2;

    let flags = prot_to_pageflags(prot);
    match vm.mprotect(addr, len, flags) {
        Ok(()) => SyscallResult::Continue(0),
        Err(e) => SyscallResult::Continue(-(e as isize) as usize),
    }
}
