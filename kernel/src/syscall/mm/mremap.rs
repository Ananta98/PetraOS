use crate::syscall::SyscallResult;
use crate::vm::vma::VmaManager;
use ostd::arch::cpu::context::UserContext;

const MREMAP_MAYMOVE: usize = 1;

pub fn syscall_mremap(
    arg0: usize,
    arg1: usize,
    arg2: usize,
    arg3: usize,
    _arg4: usize,
    _arg5: usize,
    vm: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let old_address = arg0;
    let old_size = arg1;
    let new_size = arg2;
    let flags = arg3;

    let allow_move = (flags & MREMAP_MAYMOVE) != 0;
    match vm.mremap(old_address, old_size, new_size, allow_move) {
        Ok(vaddr) => SyscallResult::Return(vaddr),
        Err(e) => SyscallResult::Return(-(e as isize) as usize),
    }
}
