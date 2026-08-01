use crate::syscall::SyscallResult;
use crate::vm::vma::VmaManager;
use ostd::arch::cpu::context::UserContext;

pub fn syscall_msync(
    arg0: usize,
    arg1: usize,
    _arg2: usize,
    _arg3: usize,
    _arg4: usize,
    _arg5: usize,
    vm: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let start = arg0;
    let length = arg1;

    match vm.msync(start, length) {
        Ok(()) => SyscallResult::Return(0),
        Err(e) => SyscallResult::Return(-(e as isize) as usize),
    }
}
