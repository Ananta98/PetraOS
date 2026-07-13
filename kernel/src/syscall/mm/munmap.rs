use crate::syscall::SyscallResult;
use crate::vm::vma::VmaManager;

pub(crate) fn syscall_munmap(
    arg0: usize,
    arg1: usize,
    _arg2: usize,
    _arg3: usize,
    _arg4: usize,
    _arg5: usize,
    vm: &VmaManager,
    _: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    let addr = arg0;
    let length = arg1;

    let result = vm.munmap(addr, length);

    match result {
        Ok(()) => SyscallResult::Continue(0),
        Err(e) => SyscallResult::Continue(-(e as isize) as usize),
    }
}
